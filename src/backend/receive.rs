use crate::interconnect::{
    AddrInfoOptions, ReceiveArgs, SendArgs, ViewProgress, ViewUpdate,
};
use anyhow::Context;
use console::style;
use data_encoding::HEXLOWER;
use futures_buffered::BufferedStreamExt;
use indicatif::{
    HumanBytes, HumanDuration, MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use iroh::{
    discovery::{dns::DnsDiscovery, pkarr::PkarrPublisher},
    Endpoint, NodeAddr, RelayMap, RelayMode, RelayUrl, SecretKey,
};
use iroh_blobs::{
    format::collection::Collection,
    get::{
        db::DownloadProgress,
        fsm::{AtBlobHeaderNextError, DecodeError},
        request::get_hash_seq_and_sizes,
    },
    net_protocol::Blobs,
    provider::{self, CustomEventSender},
    store::{ExportMode, ImportMode, ImportProgress},
    ticket::BlobTicket,
    BlobFormat, Hash, HashAndFormat, TempTag,
};
use n0_future::{future::Boxed, StreamExt};
use rand::{random, Rng};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::Instant;
use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    net::{SocketAddrV4, SocketAddrV6},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use rfd::FileHandle;
use tokio::sync::watch::{Receiver, Sender};
use walkdir::WalkDir;
use crate::backend::get_or_create_secret;

pub async fn receive(
    args: ReceiveArgs,
    view_update_sender: Sender<ViewUpdate>,
) -> anyhow::Result<()> {
    let ticket = args.ticket;
    let addr = ticket.node_addr().clone();
    let secret_key = get_or_create_secret(false)?;
    let mut builder = Endpoint::builder()
        .alpns(vec![])
        .secret_key(secret_key)
        .relay_mode(args.common.relay.into());

    if ticket.node_addr().relay_url.is_none() && ticket.node_addr().direct_addresses.is_empty() {
        builder = builder.add_discovery(|_| Some(DnsDiscovery::n0_dns()));
    }
    if let Some(addr) = args.common.magic_ipv4_addr {
        builder = builder.bind_addr_v4(addr);
    }
    if let Some(addr) = args.common.magic_ipv6_addr {
        builder = builder.bind_addr_v6(addr);
    }
    let endpoint = builder.bind().await?;
    let dir_name = format!(".sendme-get-{}", ticket.hash().to_hex());
    let iroh_data_dir = std::env::current_dir()?.join(dir_name);
    let db = iroh_blobs::store::fs::Store::load(&iroh_data_dir).await?;
    let mp = MultiProgress::new();
    let connect_progress = mp.add(ProgressBar::hidden());
    connect_progress.set_draw_target(ProgressDrawTarget::stderr());
    connect_progress.set_style(ProgressStyle::default_spinner());
    connect_progress.set_message(format!("connecting to {}", addr.node_id));
    let connection = endpoint.connect(addr, iroh_blobs::protocol::ALPN).await?;
    let hash_and_format = HashAndFormat {
        hash: ticket.hash(),
        format: ticket.format(),
    };
    connect_progress.finish_and_clear();
    let (send, recv) = async_channel::bounded(32);
    let progress = iroh_blobs::util::progress::AsyncChannelProgressSender::new(send);
    let (_hash_seq, sizes) =
        get_hash_seq_and_sizes(&connection, &hash_and_format.hash, 1024 * 1024 * 32).await?;
    // .map_err(show_get_error)?;
    let total_size = sizes.iter().sum::<u64>();
    let total_files = sizes.len().saturating_sub(1);
    // let payload_size = sizes.iter().skip(1).sum::<u64>();
    // eprintln!(
    //     "getting collection {} {} files, {}",
    //     print_hash(&ticket.hash(), args.common.format),
    //     total_files,
    //     HumanBytes(payload_size)
    // );
    // print the details of the collection only in verbose mode
    let _task = tokio::spawn(show_download_progress(
        recv,
        total_size,
        total_files,
        view_update_sender.clone(),
    ));
    let path_task = rfd::AsyncFileDialog::new()
        .set_title("Save to...")
        .save_file();
    let get_conn = || async move { Ok(connection) };
    let stats = iroh_blobs::get::db::get_to_db(&db, get_conn, &hash_and_format, progress).await?;
    view_update_sender.send(ViewUpdate::DownloadDone {
        stats: stats.clone(),
        path: String::new(),
    })?;
    // .map_err(|e| show_get_error(anyhow::anyhow!(e)))?;

    let collection = Collection::load_db(&db, &hash_and_format.hash).await?;

    if let Some((name, _)) = collection.iter().next() {
        if let Some(first) = name.split('/').next() {
            view_update_sender.send(ViewUpdate::DownloadDone {
                stats,
                path: first.to_string(),
            })?;
        }
    }
    let path = path_task.await;
    export(db, path.unwrap(), collection).await?;
    tokio::fs::remove_dir_all(iroh_data_dir).await?;

    Ok(())
}


pub async fn show_download_progress(
    recv: async_channel::Receiver<DownloadProgress>,
    total_size: u64,
    total_files: usize,
    view_update_sender: Sender<ViewUpdate>,
) -> anyhow::Result<()> {
    let mut total_done = 0;
    let mut sizes = BTreeMap::new();
    let mut last_time = Instant::now();
    let mut last_progress = 0;
    let mut speed = 0.0;
    loop {
        match recv.recv().await {
            Ok(DownloadProgress::Found { id, size, .. }) => {
                sizes.insert(id, size);
            }
            Ok(DownloadProgress::Progress { offset, .. }) => {
                let progress = total_done + offset;
                if last_time.elapsed().as_millis() > 1000 {
                    let elapsed = last_time.elapsed();
                    let progress_difference = progress.saturating_sub(last_progress);
                    speed = progress_difference as f64 / elapsed.as_secs_f64();
                    last_progress = progress;
                    last_time = Instant::now();
                }

                view_update_sender.send(ViewUpdate::Progress(ViewProgress {
                    total_size,
                    bytes_per_second: speed as u64,
                    total_files,
                    progress_value: progress,
                }))?;
            }
            Ok(DownloadProgress::Done { id }) => {
                total_done += sizes.remove(&id).unwrap_or_default();
            }
            Ok(DownloadProgress::AllDone(_)) => {
                break;
            }
            Ok(DownloadProgress::Abort(e)) => {
                anyhow::bail!("download aborted: {e:?}");
            }
            Err(e) => {
                anyhow::bail!("error reading progress: {e:?}");
            }
            _ => {}
        }
    }
    Ok(())
}

async fn export(db: impl iroh_blobs::store::Store, file_handle: FileHandle, collection: Collection) -> anyhow::Result<()> {
    let mut root = std::env::current_dir()?;
    root = file_handle.path().to_path_buf();

    for (name, hash) in collection.iter() {
        let target = get_export_path(&root, name)?;
        if target.exists() {
            eprintln!(
                "target {} already exists. Export stopped.",
                target.display()
            );
            eprintln!("You can remove the file or directory and try again. The download will not be repeated.");
            anyhow::bail!("target {} already exists", target.display());
        }
        db.export(
            *hash,
            target,
            ExportMode::TryReference,
            Box::new(move |_position| Ok(())),
        )
            .await?;
    }
    Ok(())
}


fn get_export_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let parts = name.split('/');
    let mut path = root.to_path_buf();
    for part in parts {
        validate_path_component(part)?;
        path.push(part);
    }
    Ok(path)
}

fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !component.contains('/'),
        "path components must not contain the only correct path separator, /"
    );
    Ok(())
}