#![windows_subsystem = "windows"]

use eframe::{egui::ViewportBuilder, NativeOptions};
use ui::Panel;

mod ui;

fn main() {
    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([800.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AutoDraw",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<Panel>::default())
        }),
    )
    .ok();
}
