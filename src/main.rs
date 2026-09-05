mod ai;
mod app;
mod calendar;
mod config;
mod credential;
mod notification;
mod ocr;
mod portfolio;
mod quote;
mod updater;

use app::StockWatchApp;
use chrono::Datelike;
use eframe::egui;

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app.png");

fn main() -> eframe::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--validate-data") {
        match config::load_portfolio() {
            Ok(p) => println!(
                "valid: {} accounts, {} transactions",
                p.accounts.len(),
                p.transactions.len()
            ),
            Err(error) => {
                eprintln!("{error:#}");
                std::process::exit(2);
            }
        }
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--migrate-data") {
        if let Err(error) = config::load_portfolio() {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
        let settings = config::load_settings();
        if let Err(error) = config::save_settings(&settings) {
            eprintln!("data migration failed: {error:#}");
            std::process::exit(2);
        }
        println!("data migration completed");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--update-calendar") {
        match calendar::refresh_from_sse(chrono::Local::now().year()) {
            Ok(count) => println!("updated {count} market holidays"),
            Err(error) => eprintln!("{error:#}"),
        }
        return Ok(());
    }

    if let Err(error) = config::load_portfolio() {
        rfd::MessageDialog::new()
            .set_title("持仓存档读取失败")
            .set_description(format!("{error:#}\n存档未被修改。请修正数据或从备份恢复。"))
            .set_level(rfd::MessageLevel::Error)
            .show();
        return Ok(());
    }
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
        Box::new(|cc| Ok(Box::new(StockWatchApp::new(cc)?))),
    )
}

fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).unwrap_or_default()
}
