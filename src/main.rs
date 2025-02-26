//! Command line arguments.
mod backend;
mod interconnect;
mod view;
use crate::view::View;

fn main() -> anyhow::Result<()> {
    // tracing_subscriber::fmt::init();
    let res = eframe::run_native(
        "Send Me View",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([400.0, 300.0])
                .with_min_inner_size([300.0, 220.0])
                .with_always_on_top(),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(View::default()))),
    );
    match res {
        Ok(()) => std::process::exit(0),
        Err(_) => std::process::exit(1),
    }
}
