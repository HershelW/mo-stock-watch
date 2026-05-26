mod ai;
mod app;
mod config;
mod ocr;
mod portfolio;
mod quote;

use app::StockWatchApp;
use eframe::egui;

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app.png");

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("持仓")
            .with_icon(app_icon())
            .with_inner_size([720.0, 420.0])
            .with_min_inner_size([170.0, 72.0])
            .with_decorations(true)
            .with_transparent(false),
        ..Default::default()
    };

    eframe::run_native(
        "持仓",
        options,
        Box::new(|cc| Ok(Box::new(StockWatchApp::new(cc)))),
    )
}

fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).unwrap_or_default()
}
