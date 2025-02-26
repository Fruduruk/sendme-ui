use crate::backend::{receive, send};
use crate::interconnect::{
    AddrInfoOptions, CommonArgs, Format, ReceiveArgs, RelayModeOption, SendArgs,
};
use clap::Parser;
use egui::{Context, Ui};
use iroh_blobs::ticket::BlobTicket;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

pub struct View {
    pub init: bool,
    pub path: String,
    pub ticket: String,
    pub sending_handle: Option<JoinHandle<anyhow::Result<()>>>,
    pub receiving_handle: Option<JoinHandle<()>>,
    pub tokio_runtime: Runtime,
}

impl View {
    fn init(&mut self, ctx: &Context) {
        ctx.set_pixels_per_point(2.0);
        self.init = false;
    }
}

impl eframe::App for View {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.init {
            self.init(ctx);
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::widgets::global_theme_preference_buttons(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_send_ui(ui);
            self.show_receive_ui(ui);
        });
    }
}

impl View {
    fn show_send_ui(&mut self, ui: &mut Ui) {
        if let Some(handle) = &self.sending_handle {
            if handle.is_finished() {
                self.sending_handle = None;
            } else {
                // if ui.button("Cancel").clicked() {
                //
                // }
            }
        } else {
            ui.label("Insert path to your file or directory");
            egui::TextEdit::multiline(&mut self.path)
                .hint_text("C:\\...   or \"C:\\...\"")
                .show(ui);
            let clean_path = remove_quotes(&self.path);
            self.path = clean_path.into();

            if ui.button("Send").clicked() {
                let args = SendArgs {
                    path: PathBuf::from(self.path.clone()),
                    common: CommonArgs::default(),
                    ticket_type: AddrInfoOptions::default(),
                };
                let task = self.tokio_runtime.spawn(async move { send(args).await });
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
                let task = self.tokio_runtime.spawn_blocking(move || {
                    handle.block_on(async {
                        receive(args).await.unwrap();
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
