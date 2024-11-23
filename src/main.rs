#![windows_subsystem = "windows"]

use eframe::{egui::ViewportBuilder, NativeOptions};
use ui::Panel;

mod font;
mod ui;

rust_i18n::i18n!("i18n");

fn main() {
    rust_i18n::set_locale("zh-CN");
    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([800.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AutoDraw",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Panel::new(cc))
        }),
    )
    .ok();
}
