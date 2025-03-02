pub mod send;
pub mod receive;

use crate::interconnect::{ AddrInfoOptions, Format};
use anyhow::Context;
use futures_buffered::BufferedStreamExt;
use iroh::{
    NodeAddr, SecretKey,
};
use iroh_blobs::{
    provider::{CustomEventSender},
     Hash,
};
use n0_future::{ StreamExt};
use rand::{Rng};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::{
    fmt::{Display},
    str::FromStr,
};

/// Get the secret key or generate a new one.
///
/// Print the secret key to stderr if it was generated, so the user can save it.
fn get_or_create_secret(print: bool) -> anyhow::Result<SecretKey> {
    match std::env::var("IROH_SECRET") {
        Ok(secret) => SecretKey::from_str(&secret).context("invalid secret"),
        Err(_) => {
            let key = SecretKey::generate(rand::rngs::OsRng);
            if print {
                eprintln!("using secret key {}", key);
            }
            Ok(key)
        }
    }
}

pub fn apply_options(addr: &mut NodeAddr, opts: AddrInfoOptions) {
    match opts {
        AddrInfoOptions::Id => {
            addr.direct_addresses.clear();
            addr.relay_url = None;
        }
        AddrInfoOptions::RelayAndAddresses => {
            // nothing to do
        }
        AddrInfoOptions::Relay => {
            addr.direct_addresses.clear();
        }
        AddrInfoOptions::Addresses => {
            addr.relay_url = None;
        }
    }
}


pub fn print_hash(hash: &Hash, format: Format) -> String {
    match format {
        Format::Hex => hash.to_hex().to_string(),
        Format::Cid => hash.to_string(),
    }
}





// #[derive(Debug, Clone)]
// struct SendStatus {
//     /// the multiprogress bar
//     mp: MultiProgress,
// }
//
// impl SendStatus {
//     fn new() -> Self {
//         let mp = MultiProgress::new();
//         mp.set_draw_target(ProgressDrawTarget::stderr());
//         Self { mp }
//     }
//
//     fn new_client(&self) -> ClientStatus {
//         let current = self.mp.add(ProgressBar::hidden());
//         current.set_style(
//             ProgressStyle::default_spinner()
//                 .template("{spinner:.green} [{elapsed_precise}] {msg}")
//                 .unwrap(),
//         );
//         current.enable_steady_tick(Duration::from_millis(100));
//         current.set_message("waiting for requests");
//         ClientStatus {
//             current: current.into(),
//         }
//     }
// }

// #[derive(Debug, Clone)]
// struct ClientStatus {
//     current: Arc<ProgressBar>,
// }
//
// impl Drop for ClientStatus {
//     fn drop(&mut self) {
//         if Arc::strong_count(&self.current) == 1 {
//             self.current.finish_and_clear();
//         }
//     }
// }
//
// impl CustomEventSender for ClientStatus {
//     fn send(&self, event: iroh_blobs::provider::Event) -> Boxed<()> {
//         self.try_send(event);
//         Box::pin(std::future::ready(()))
//     }
//
//     fn try_send(&self, event: provider::Event) {
//         tracing::info!("{:?}", event);
//         let msg = match event {
//             provider::Event::ClientConnected { connection_id } => {
//                 Some(format!("{} got connection", connection_id))
//             }
//             provider::Event::TransferBlobCompleted {
//                 connection_id,
//                 hash,
//                 index,
//                 size,
//                 ..
//             } => Some(format!(
//                 "{} transfer blob completed {} {} {}",
//                 connection_id,
//                 hash,
//                 index,
//                 HumanBytes(size)
//             )),
//             provider::Event::TransferCompleted {
//                 connection_id,
//                 stats,
//                 ..
//             } => Some(format!(
//                 "{} transfer completed {} {}",
//                 connection_id,
//                 stats.send.write_bytes.size,
//                 HumanDuration(stats.send.write_bytes.stats.duration)
//             )),
//             provider::Event::TransferAborted { connection_id, .. } => {
//                 Some(format!("{} transfer completed", connection_id))
//             }
//             _ => None,
//         };
//         if let Some(msg) = msg {
//             self.current.set_message(msg);
//         }
//     }
// }



// fn show_get_error(e: anyhow::Error) -> anyhow::Error {
//     if let Some(err) = e.downcast_ref::<DecodeError>() {
//         match err {
//             DecodeError::NotFound => {
//                 eprintln!("{}", style("send side no longer has a file").yellow())
//             }
//             DecodeError::LeafNotFound(_) | DecodeError::ParentNotFound(_) => eprintln!(
//                 "{}",
//                 style("send side no longer has part of a file").yellow()
//             ),
//             DecodeError::Io(err) => eprintln!(
//                 "{}",
//                 style(format!("generic network error: {}", err)).yellow()
//             ),
//             DecodeError::Read(err) => eprintln!(
//                 "{}",
//                 style(format!("error reading data from quinn: {}", err)).yellow()
//             ),
//             DecodeError::LeafHashMismatch(_) | DecodeError::ParentHashMismatch(_) => {
//                 eprintln!("{}", style("send side sent wrong data").red())
//             }
//         };
//     } else if let Some(header_error) = e.downcast_ref::<AtBlobHeaderNextError>() {
//         // TODO(iroh-bytes): get_to_db should have a concrete error type so you don't have to guess
//         match header_error {
//             AtBlobHeaderNextError::Io(err) => eprintln!(
//                 "{}",
//                 style(format!("generic network error: {}", err)).yellow()
//             ),
//             AtBlobHeaderNextError::Read(err) => eprintln!(
//                 "{}",
//                 style(format!("error reading data from quinn: {}", err)).yellow()
//             ),
//             AtBlobHeaderNextError::NotFound => {
//                 eprintln!("{}", style("send side no longer has a file").yellow())
//             }
//         };
//     } else {
//         eprintln!(
//             "{}",
//             style(format!("generic error: {:?}", e.root_cause())).red()
//         );
//     }
//     e
// }

