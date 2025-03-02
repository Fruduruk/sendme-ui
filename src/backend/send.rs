use crate::interconnect::{
     AddrInfoOptions, SendArgs,  ViewUpdate,
};
use anyhow::Context;
use console::style;
use data_encoding::HEXLOWER;
use futures_buffered::BufferedStreamExt;
use indicatif::{
    HumanBytes,MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use iroh::{
    discovery::{ pkarr::PkarrPublisher},
    Endpoint,
};
use iroh_blobs::{
    format::collection::Collection,
    net_protocol::Blobs,
    provider::{ CustomEventSender},
    store::{ ImportMode, ImportProgress},
    ticket::BlobTicket,
    BlobFormat,TempTag,
};
use n0_future::{StreamExt};
use rand::{random, Rng};
use std::{
    collections::BTreeMap,
    fmt::{Display},
    path::{Component, Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use tokio::sync::watch::{Receiver, Sender};
use walkdir::WalkDir;
use crate::backend::{apply_options, get_or_create_secret, print_hash};

pub async fn send(
    args: SendArgs,
    view_update_sender: Sender<ViewUpdate>,
    mut cancel_receiver: Receiver<bool>,
) -> anyhow::Result<()> {
    let secret_key = get_or_create_secret(false)?;
    // create a magicsocket endpoint
    let mut builder = Endpoint::builder()
        .alpns(vec![iroh_blobs::protocol::ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(args.common.relay.into());
    if args.ticket_type == AddrInfoOptions::Id {
        builder =
            builder.add_discovery(|secret_key| Some(PkarrPublisher::n0_dns(secret_key.clone())));
    }
    if let Some(addr) = args.common.magic_ipv4_addr {
        builder = builder.bind_addr_v4(addr);
    }
    if let Some(addr) = args.common.magic_ipv6_addr {
        builder = builder.bind_addr_v6(addr);
    }

    // use a flat store - todo: use a partial in mem store instead
    let suffix = random::<[u8; 16]>();
    let cwd = std::env::current_dir()?;
    let blobs_data_dir = cwd.join(format!(".sendme-send-{}", HEXLOWER.encode(&suffix)));
    if blobs_data_dir.exists() {
        println!(
            "can not share twice from the same directory: {}",
            cwd.display(),
        );
        std::process::exit(1);
    }

    tokio::fs::create_dir_all(&blobs_data_dir).await?;

    let endpoint = builder.bind().await?;
    // let ps = SendStatus::new();
    let blobs = Blobs::persistent(&blobs_data_dir)
        .await?
        // .events(ps.new_client().into())
        .build(&endpoint);

    let router = iroh::protocol::Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs.clone())
        .spawn()
        .await?;

    let path = args.path;
    let (temp_tag, size, collection) = import(path.clone(), blobs.store().clone()).await?;
    let hash = *temp_tag.hash();

    // wait for the endpoint to figure out its address before making a ticket
    let _ = router.endpoint().home_relay().initialized().await?;

    // make a ticket
    let mut addr = router.endpoint().node_addr().await?;
    apply_options(&mut addr, args.ticket_type);
    let ticket = BlobTicket::new(addr, hash, BlobFormat::HashSeq)?;
    let entry_type = if path.is_file() { "file" } else { "directory" };
    println!(
        "imported {} {}, {}, hash {}",
        entry_type,
        path.display(),
        HumanBytes(size),
        print_hash(&hash, args.common.format)
    );
    view_update_sender.send(ViewUpdate::Ticket(ticket.clone()))?;

    drop(temp_tag);

    // Wait for exit
    // tokio::signal::ctrl_c().await?;
    while cancel_receiver.changed().await.is_ok() {
        if *cancel_receiver.borrow() {
            break;
        }
    }

    println!("shutting down");
    tokio::time::timeout(Duration::from_secs(2), router.shutdown()).await??;
    tokio::fs::remove_dir_all(blobs_data_dir).await?;
    view_update_sender.send(ViewUpdate::Nothing)?;

    Ok(())
}

/// Import from a file or directory into the database.
///
/// The returned tag always refers to a collection. If the input is a file, this
/// is a collection with a single blob, named like the file.
///
/// If the input is a directory, the collection contains all the files in the
/// directory.
async fn import(
    path: PathBuf,
    db: impl iroh_blobs::store::Store,
) -> anyhow::Result<(TempTag, u64, Collection)> {
    let path = path.canonicalize()?;
    anyhow::ensure!(path.exists(), "path {} does not exist", path.display());
    let root = path.parent().context("context get parent")?;
    // walkdir also works for files, so we don't need to special case them
    let files = WalkDir::new(path.clone()).into_iter();
    // flatten the directory structure into a list of (name, path) pairs.
    // ignore symlinks.
    let data_sources: Vec<(String, PathBuf)> = files
        .map(|entry| {
            let entry = entry?;
            if !entry.file_type().is_file() {
                // Skip symlinks. Directories are handled by WalkDir.
                return Ok(None);
            }
            let path = entry.into_path();
            let relative = path.strip_prefix(root)?;
            let name = canonical_path_to_string(relative, true)?;
            anyhow::Ok(Some((name, path)))
        })
        .filter_map(Result::transpose)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let (send, recv) = async_channel::bounded(32);
    let progress = iroh_blobs::util::progress::AsyncChannelProgressSender::new(send);
    let show_progress = tokio::spawn(show_ingest_progress(recv));
    // import all the files, using num_cpus workers, return names and temp tags
    let mut names_and_tags = futures_lite::stream::iter(data_sources)
        .map(|(name, path)| {
            let db = db.clone();
            let progress = progress.clone();
            async move {
                let (temp_tag, file_size) = db
                    .import_file(path, ImportMode::TryReference, BlobFormat::Raw, progress)
                    .await?;
                anyhow::Ok((name, temp_tag, file_size))
            }
        })
        .buffered_unordered(num_cpus::get())
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;
    drop(progress);
    names_and_tags.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
    // total size of all files
    let size = names_and_tags.iter().map(|(_, _, size)| *size).sum::<u64>();
    // collect the (name, hash) tuples into a collection
    // we must also keep the tags around so the data does not get gced.
    let (collection, tags) = names_and_tags
        .into_iter()
        .map(|(name, tag, _)| ((name, *tag.hash()), tag))
        .unzip::<_, _, Collection, Vec<_>>();
    let temp_tag = collection.clone().store(&db).await?;
    // now that the collection is stored, we can drop the tags
    // data is protected by the collection
    drop(tags);
    show_progress.await??;
    Ok((temp_tag, size, collection))
}

pub async fn show_ingest_progress(
    recv: async_channel::Receiver<ImportProgress>,
) -> anyhow::Result<()> {
    let mp = MultiProgress::new();
    mp.set_draw_target(ProgressDrawTarget::stderr());
    let op = mp.add(ProgressBar::hidden());
    op.set_style(
        ProgressStyle::default_spinner().template("{spinner:.green} [{elapsed_precise}] {msg}")?,
    );
    // op.set_message(format!("{} Ingesting ...\n", style("[1/2]").bold().dim()));
    // op.set_length(total_files);
    let mut names = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut pbs = BTreeMap::new();
    loop {
        let event = recv.recv().await;
        match event {
            Ok(ImportProgress::Found { id, name }) => {
                names.insert(id, name);
            }
            Ok(ImportProgress::Size { id, size }) => {
                sizes.insert(id, size);
                let total_size = sizes.values().sum::<u64>();
                op.set_message(format!(
                    "{} Ingesting {} files, {}\n",
                    style("[1/2]").bold().dim(),
                    sizes.len(),
                    HumanBytes(total_size)
                ));
                let name = names.get(&id).cloned().unwrap_or_default();
                let pb = mp.add(ProgressBar::hidden());
                pb.set_style(ProgressStyle::with_template(
                    "{msg}{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
                )?.progress_chars("#>-"));
                pb.set_message(format!("{} {}", style("[2/2]").bold().dim(), name));
                pb.set_length(size);
                pbs.insert(id, pb);
            }
            Ok(ImportProgress::OutboardProgress { id, offset }) => {
                if let Some(pb) = pbs.get(&id) {
                    pb.set_position(offset);
                }
            }
            Ok(ImportProgress::OutboardDone { id, .. }) => {
                // you are not guaranteed to get any OutboardProgress
                if let Some(pb) = pbs.remove(&id) {
                    pb.finish_and_clear();
                }
            }
            Ok(ImportProgress::CopyProgress { .. }) => {
                // we are not copying anything
            }
            Err(e) => {
                op.set_message(format!("Error receiving progress: {e}"));
                break;
            }
        }
    }
    op.finish_and_clear();
    Ok(())
}

/// This function converts an already canonical path to a string.
///
/// If `must_be_relative` is true, the function will fail if any component of the path is
/// `Component::RootDir`
///
/// This function will also fail if the path is non-canonical, i.e. contains
/// `..` or `.`, or if the path components contain any windows or unix path
/// separators.
pub fn canonical_path_to_string(
    path: impl AsRef<Path>,
    must_be_relative: bool,
) -> anyhow::Result<String> {
    let mut path_str = String::new();
    let parts = path
        .as_ref()
        .components()
        .filter_map(|c| match c {
            Component::Normal(x) => {
                let c = match x.to_str() {
                    Some(c) => c,
                    None => return Some(Err(anyhow::anyhow!("invalid character in path"))),
                };

                if !c.contains('/') && !c.contains('\\') {
                    Some(Ok(c))
                } else {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                }
            }
            Component::RootDir => {
                if must_be_relative {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                } else {
                    path_str.push('/');
                    None
                }
            }
            _ => Some(Err(anyhow::anyhow!("invalid path component {:?}", c))),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let parts = parts.join("/");
    path_str.push_str(&parts);
    Ok(path_str)
}
