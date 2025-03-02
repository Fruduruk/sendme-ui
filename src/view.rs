use crate::interconnect::{AddrInfoOptions, CommonArgs, ReceiveArgs, SendArgs, ViewUpdate};
use arboard::Clipboard;
use egui::{Context, ProgressBar, Ui};
use indicatif::{HumanBytes, HumanDuration};
use iroh_blobs::ticket::BlobTicket;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::runtime::Runtime;
use tokio::sync::watch::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use crate::backend::receive::receive;
use crate::backend::send::send;

enum Tab {
    Send,
    Receive,
}

pub struct View {
    init: bool,
    tab: Tab,
    path: String,
    ticket: String,
    sending_handle: Option<JoinHandle<anyhow::Result<()>>>,
    receiving_handle: Option<JoinHandle<()>>,
    tokio_runtime: Runtime,
    receiver: Receiver<ViewUpdate>,
    sender: Sender<ViewUpdate>,
    cancel_sender: Sender<bool>,
    cancel_receiver: Receiver<bool>,
}

impl Default for View {
    fn default() -> Self {
        let (sender, receiver) = channel(ViewUpdate::Nothing);
        let (cancel_sender, cancel_receiver) = channel(false);
        View {
            init: true,
            tab: Tab::Send,
            path: String::new(),
            ticket: String::new(),
            sending_handle: None,
            receiving_handle: None,
            tokio_runtime: Runtime::new().unwrap(),
            sender,
            receiver,
            cancel_sender,
            cancel_receiver,
        }
    }
}

impl eframe::App for View {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.init {
            self.init(ctx);
        }
        ctx.request_repaint();

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::widgets::global_theme_preference_buttons(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sending_handle.is_none() && self.receiving_handle.is_none() {
                ui.horizontal(|ui| {
                    if ui.button("Send Page").clicked() {
                        self.tab = Tab::Send;
                    }
                    if ui.button("Receive Page").clicked() {
                        self.tab = Tab::Receive;
                    }
                });
            }
            match self.tab {
                Tab::Send => {
                    self.show_send_ui(ctx,ui);
                }
                Tab::Receive => {
                    self.show_receive_ui(ui);
                }
            }
            self.show_results(ui);
        });
    }
}

impl View {
    fn init(&mut self, ctx: &Context) {
        ctx.set_pixels_per_point(2.0);
        self.init = false;
    }

    fn show_results(&mut self, ui: &mut Ui) {
        match self.receiver.borrow().deref() {
            ViewUpdate::Nothing => {}
            ViewUpdate::Ticket(ticket) => {
                Self::show_ticket(ui, ticket);
            }
            ViewUpdate::Progress(view_progress) => {
                let progress =
                    view_progress.progress_value as f32 / view_progress.total_size as f32;
                let bar = ProgressBar::new(progress);
                ui.add(bar);
                ui.label(format!(
                    "Downloading... {}/{}  {}/s",
                    HumanBytes(view_progress.progress_value),
                    HumanBytes(view_progress.total_size),
                    HumanBytes(view_progress.bytes_per_second)
                ));
            }
            ViewUpdate::DownloadDone{stats, path} => {
                ui.label(format!(
                    "Transferred {} in {}, {}/s",
                    HumanBytes(stats.bytes_read),
                    HumanDuration(stats.elapsed),
                    HumanBytes((stats.bytes_read as f64 / stats.elapsed.as_secs_f64()) as u64)
                ));
                ui.label(format!("Saved to {path}"));
            }
        }
    }

    fn show_ticket(ui: &mut Ui, ticket: &BlobTicket) {
        ui.label(format!("Generated ticket: {}", ticket));
        if ui.button("Copy to clipboard").clicked() {
            let mut clipboard = Clipboard::new().unwrap();
            clipboard.clear().unwrap();
            clipboard.set_text(ticket.to_string()).unwrap();
        }
    }

    fn show_send_ui(&mut self,ctx: &Context, ui: &mut Ui) {
        if let Some(handle) = &self.sending_handle {
            if handle.is_finished() {
                self.sending_handle = None;
            } else {
                if ui.button("Cancel").clicked() {
                    self.cancel_sender.send(true).unwrap();
                }
            }
        } else {
            ui.label("Insert path to your file or directory");
            egui::TextEdit::multiline(&mut self.path)
                .hint_text("C:\\...   or \"C:\\...\"")
                .show(ui);
            let clean_path = remove_quotes(&self.path);
            self.path = clean_path.into();

            ctx.input(|i| {
                for file in &i.raw.dropped_files {
                    if let Some(path) = &file.path {
                        self.path = path.to_str().unwrap().into();
                    }
                }
            });
            
            if ui.button("Send").clicked() {
                self.cancel_sender.send(false).unwrap();
                let args = SendArgs {
                    path: PathBuf::from(self.path.clone()),
                    common: CommonArgs::default(),
                    ticket_type: AddrInfoOptions::default(),
                };
                let sender = self.sender.clone();
                let cancel_receiver = self.cancel_receiver.clone();
                let task = self
                    .tokio_runtime
                    .spawn(async move { send(args, sender, cancel_receiver).await });
                self.sending_handle = Some(task);
            }
        }
    }

    fn show_receive_ui(&mut self, ui: &mut Ui) {
        if let Some(handle) = &self.receiving_handle {
            if handle.is_finished() {
                self.receiving_handle = None;
            }
        } else {
            ui.label("Insert ticket to receive data.");
            egui::TextEdit::multiline(&mut self.ticket)
                .hint_text("blobabcdefg....")
                .show(ui);

            if ui.button("Receive").clicked() {
                let args = ReceiveArgs {
                    common: CommonArgs::default(),
                    ticket: BlobTicket::from_str(&self.ticket).unwrap(),
                };

                let handle = self.tokio_runtime.handle().clone();
                let sender = self.sender.clone();
                let task = self.tokio_runtime.spawn_blocking(move || {
                    handle.block_on(async {
                        receive(args, sender).await.unwrap();
                    })
                });

                self.receiving_handle = Some(task);
            }
        }
    }
}

fn remove_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}
