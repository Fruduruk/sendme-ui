use crate::{receive, send, ReceiveArgs, SendArgs};
use clap::Parser;
use egui::{Context, Ui};
use tokio::runtime::Runtime;
use tokio::task::Id;
use tokio::task::JoinHandle;

pub struct View {
    pub path: String,
    pub ticket: String,
    pub sending_handle: Option<JoinHandle<anyhow::Result<()>>>,
    pub receiving_handle: Option<JoinHandle<()>>,
    pub tokio_runtime: Runtime
}

impl eframe::App for View {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
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
                let args = SendArgs::parse_from(vec!["send".into(), self.path.clone()]);
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
                let args = ReceiveArgs::parse_from(vec!["receive".into(), self.ticket.clone()]);

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
