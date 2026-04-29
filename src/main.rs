mod ai;
mod app;
mod config;
mod ocr;
mod portfolio;
mod quote;

use app::StockWatchApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("持仓")
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
