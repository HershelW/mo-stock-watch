use crate::{
    ai,
    config::{self, AppSettings},
    ocr,
    portfolio::{Holding, Market, Portfolio},
    quote::{self, QuoteBook, QuoteFetchResult},
};
use chrono::{Datelike, Local, NaiveDateTime, NaiveTime, Weekday};
use eframe::egui::{
    self, vec2, Align, Color32, FontData, FontFamily, FontId, Frame, Grid, Key, Layout, Rect,
    RichText, ScrollArea, Sense, Stroke, TextStyle, ViewportCommand,
};
use image::{ImageBuffer, Rgba};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

const NORMAL_WINDOW_SIZE: [f32; 2] = [720.0, 420.0];
const COMPACT_WINDOW_SIZE: [f32; 2] = [252.0, 112.0];

pub struct StockWatchApp {
    portfolio: Portfolio,
    settings: AppSettings,
    quotes: QuoteBook,
    fetch_rx: Option<Receiver<anyhow::Result<QuoteFetchResult>>>,
    ai_ocr_rx: Option<Receiver<anyhow::Result<Vec<Holding>>>>,
    next_refresh_at: Instant,
    quote_failure_count: u32,
    status: String,
    editing: bool,
    show_ocr_panel: bool,
    show_toolbar: bool,
}

impl StockWatchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let mut portfolio = config::load_portfolio();
        portfolio.normalize();
        let settings = config::load_settings();
        let mut app = Self {
            portfolio,
            settings,
            quotes: QuoteBook::default(),
            fetch_rx: None,
            ai_ocr_rx: None,
            next_refresh_at: Instant::now(),
            quote_failure_count: 0,
            status: "准备刷新行情".to_owned(),
            editing: false,
            show_ocr_panel: false,
            show_toolbar: false,
        };
        if app.settings.ultra_compact {
            cc.egui_ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                COMPACT_WINDOW_SIZE[0],
                COMPACT_WINDOW_SIZE[1],
            )));
        }
        app.start_fetch();
        app
    }

    fn start_fetch(&mut self) {
        if self.fetch_rx.is_some() {
            return;
        }
        if let Some(delay) = delay_until_next_market_session(Local::now()) {
            self.quotes.loading = false;
            self.next_refresh_at = Instant::now() + delay;
            self.status = format!("非交易时段，{} 后刷新", format_duration_for_status(delay));
            return;
        }
        self.portfolio.normalize();
        self.quotes.loading = true;
        self.fetch_rx = Some(quote::spawn_fetch(self.portfolio.holdings.clone()));
        self.status = "正在刷新东方财富行情...".to_owned();
    }

    fn poll_fetch(&mut self) {
        let Some(rx) = &self.fetch_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                let updated_at = result
                    .quotes
                    .first()
                    .map(|quote| quote.updated_at)
                    .unwrap_or_else(Local::now);
                self.quotes.quotes = result
                    .quotes
                    .into_iter()
                    .map(|q| (q.code.clone(), q))
                    .collect();
                self.quotes.last_updated_at = Some(updated_at);
                self.quotes.last_error = None;
                self.quotes.loading = false;
                self.fetch_rx = None;
                self.quote_failure_count = 0;
                self.next_refresh_at = Instant::now() + self.normal_refresh_delay();
                self.status = format!("行情已更新（{}）", result.source.label());
            }
            Ok(Err(err)) => {
                self.quotes.last_error = Some(err.to_string());
                self.quotes.loading = false;
                self.fetch_rx = None;
                self.quote_failure_count = self.quote_failure_count.saturating_add(1);
                let delay = self.failure_refresh_delay();
                self.next_refresh_at = Instant::now() + delay;
                self.status = format!("行情刷新失败：{err:#}；{}s 后重试", delay.as_secs());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.fetch_rx = None;
                self.quotes.loading = false;
                self.status = "行情线程已断开".to_owned();
            }
        }
    }

    fn normal_refresh_delay(&self) -> Duration {
        Duration::from_secs(self.settings.refresh_interval_secs.max(5))
    }

    fn failure_refresh_delay(&self) -> Duration {
        let secs = match self.quote_failure_count {
            0 | 1 => 30,
            2 => 60,
            _ => 120,
        };
        Duration::from_secs(secs)
    }

    fn poll_ai_ocr(&mut self) {
        let Some(rx) = &self.ai_ocr_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(holdings)) if holdings.is_empty() => {
                self.ai_ocr_rx = None;
                self.status = "AI 没有从截图中识别出可用持仓行".to_owned();
            }
            Ok(Ok(mut holdings)) => {
                self.ai_ocr_rx = None;
                self.portfolio.holdings.append(&mut holdings);
                self.portfolio.normalize();
                self.editing = true;
                match config::save_portfolio(&mut self.portfolio) {
                    Ok(_) => self.status = "AI 已导入并自动保存，请继续核对".to_owned(),
                    Err(err) => self.status = format!("AI 已导入，但自动保存失败：{err:#}"),
                }
            }
            Ok(Err(err)) => {
                self.ai_ocr_rx = None;
                self.status = format!("AI OCR 失败：{err:#}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ai_ocr_rx = None;
                self.status = "AI OCR 线程已断开".to_owned();
            }
        }
    }

    fn save_all(&mut self) {
        match config::save_portfolio(&mut self.portfolio)
            .and_then(|_| config::save_settings(&self.settings))
        {
            Ok(_) => self.status = "已保存本地配置".to_owned(),
            Err(err) => self.status = format!("保存失败：{err:#}"),
        }
    }

    fn add_empty_holding(&mut self) {
        self.portfolio.holdings.push(Holding {
            code: String::new(),
            name: String::new(),
            quantity: 0.0,
            cost_price: 0.0,
            market: Market::Shenzhen,
        });
        self.editing = true;
    }

    fn import_ocr(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("截图", &["png", "jpg", "jpeg", "bmp"])
            .pick_file()
        else {
            return;
        };

        self.import_ocr_path(path);
    }

    fn import_ocr_path(&mut self, path: PathBuf) {
        match ocr::recognize_holdings_from_image(&path) {
            Ok(holdings) if holdings.is_empty() => {
                self.status = "没有从截图中识别出可用持仓行".to_owned();
            }
            Ok(mut holdings) => {
                self.portfolio.holdings.append(&mut holdings);
                self.portfolio.normalize();
                self.editing = true;
                match config::save_portfolio(&mut self.portfolio) {
                    Ok(_) => self.status = "已导入 OCR 草稿并自动保存，请逐行核对".to_owned(),
                    Err(err) => self.status = format!("已导入 OCR 草稿，但自动保存失败：{err:#}"),
                }
            }
            Err(err) => self.status = format!("{err:#}"),
        }
    }

    fn import_ai_ocr_clipboard(&mut self) {
        self.status = "已触发粘贴，正在读取剪贴板图片...".to_owned();
        match self.clipboard_image_path() {
            Ok(path) => self.import_ai_ocr_path(path),
            Err(err) => self.status = format!("读取剪贴板图片失败：{err:#}"),
        }
    }

    fn clipboard_image_path(&mut self) -> anyhow::Result<PathBuf> {
        let path = (|| -> anyhow::Result<PathBuf> {
            let mut clipboard = arboard::Clipboard::new()?;
            let image = clipboard.get_image()?;
            fs::create_dir_all(config::app_dir())?;
            let path = config::app_dir().join("clipboard_ocr.png");
            let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(
                image.width as u32,
                image.height as u32,
                image.bytes.into_owned(),
            )
            .ok_or_else(|| anyhow::anyhow!("剪贴板图片格式无法转换"))?;
            buffer.save(&path)?;
            Ok(path)
        })();

        path
    }

    fn check_clipboard_image(&mut self) {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image()) {
            Ok(image) => {
                self.status = format!("剪贴板里有图片：{} x {}", image.width, image.height);
            }
            Err(err) => {
                self.status = format!("剪贴板没有可读取图片：{err}");
            }
        }
    }

    fn import_ai_ocr_path(&mut self, path: PathBuf) {
        if self.ai_ocr_rx.is_some() {
            self.status = "AI 正在识别上一张截图，请稍等".to_owned();
            return;
        }

        let api_key = self.settings.openai_api_key.clone();
        let base_url = self.settings.openai_base_url.clone();
        let model = self.settings.ocr_model.clone();
        let status_model = model.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = ai::recognize_holdings_with_openai(&api_key, &base_url, &model, &path);
            let _ = tx.send(result);
        });
        self.status = format!("正在用 {status_model} 识别截图...");
        self.ai_ocr_rx = Some(rx);
    }

    fn totals(&self) -> (f64, f64, f64) {
        self.portfolio
            .holdings
            .iter()
            .fold((0.0, 0.0, 0.0), |acc, h| {
                if let Some(q) = self.quotes.quotes.get(&h.code) {
                    (
                        acc.0 + q.market_value(h),
                        acc.1 + q.position_pnl(h),
                        acc.2 + q.today_pnl(h),
                    )
                } else {
                    acc
                }
            })
    }

    fn pnl_color(&self, value: f64) -> Color32 {
        pnl_color_for(value)
    }

    fn set_ultra_compact(&mut self, ctx: &egui::Context, compact: bool) {
        if compact {
            if let Some(size) = ctx.input(|i| i.viewport().inner_rect.map(|rect| rect.size())) {
                if size.x > COMPACT_WINDOW_SIZE[0] + 24.0
                    || size.y > COMPACT_WINDOW_SIZE[1] + 24.0
                {
                    self.settings.normal_window_size = Some([size.x, size.y]);
                }
            }
            self.settings.ultra_compact = true;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                COMPACT_WINDOW_SIZE[0],
                COMPACT_WINDOW_SIZE[1],
            )));
        } else {
            self.settings.ultra_compact = false;
            let size = self.settings.normal_window_size.unwrap_or(NORMAL_WINDOW_SIZE);
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(size[0], size[1])));
        }
        let _ = config::save_settings(&self.settings);
    }
}

impl eframe::App for StockWatchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_fetch();
        self.poll_ai_ocr();
        if Instant::now() >= self.next_refresh_at && !self.editing {
            self.start_fetch();
        }
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
            if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));
        ctx.request_repaint_after(Duration::from_millis(250));
        apply_text_scale(ctx, self.settings.font_scale);
        self.handle_dropped_files(ctx);
        self.handle_ocr_shortcuts(ctx);

        let bg = Color32::from_rgb(14, 17, 22);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(bg).inner_margin(12.0))
            .show(ctx, |ui| {
                if self.settings.ultra_compact {
                    self.render_ultra_compact(ui, ctx);
                    return;
                }

                self.render_header(ui, ctx);
                ui.add_space(8.0);
                self.render_toolbar_toggle(ui);
                if self.show_toolbar {
                    ui.add_space(6.0);
                    self.render_toolbar(ui);
                }
                if self.show_ocr_panel {
                    ui.add_space(8.0);
                    self.render_ocr_panel(ui);
                }
                ui.add_space(8.0);
                self.render_holdings(ui);
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&self.status)
                                .color(Color32::from_gray(180))
                                .small(),
                        );
                    });
                });
            });
    }
}

impl StockWatchApp {
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if !self.show_ocr_panel {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                self.import_ocr_path(path);
                break;
            }
        }
    }

    fn handle_ocr_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.show_ocr_panel {
            return;
        }

        let paste_pressed = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::V));
        if paste_pressed {
            self.status = "收到 Ctrl+V".to_owned();
            self.import_ai_ocr_clipboard();
        }
    }

    fn render_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (value, total_pnl, today_pnl) = self.totals();
        Frame::new()
            .fill(Color32::from_rgb(19, 23, 30))
            .stroke(Stroke::new(1.0, Color32::from_rgb(36, 43, 54)))
            .corner_radius(10.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let time = Local::now().format("%H:%M:%S").to_string();
                    ui.label(
                        RichText::new(format!("A股 · {time}"))
                            .color(Color32::from_gray(174))
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format_money(today_pnl))
                                .color(self.pnl_color(today_pnl))
                                .strong(),
                        );
                        ui.label(RichText::new("今日").color(Color32::from_gray(132)));
                        if compact_toggle_button(ui, self.settings.ultra_compact).clicked() {
                            self.set_ultra_compact(ctx, true);
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    metric_card(
                        ui,
                        "市值",
                        value,
                        Color32::from_gray(235),
                        self.settings.font_scale,
                    );
                    metric_card(
                        ui,
                        "持仓盈亏",
                        total_pnl,
                        self.pnl_color(total_pnl),
                        self.settings.font_scale,
                    );
                    metric_card(
                        ui,
                        "今日浮盈",
                        today_pnl,
                        self.pnl_color(today_pnl),
                        self.settings.font_scale,
                    );
                });
            });
    }

    fn render_ultra_compact(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (_, _, today_pnl) = self.totals();
        Frame::new()
            .fill(Color32::from_rgb(19, 23, 30))
            .stroke(Stroke::new(1.0, Color32::from_rgb(36, 43, 54)))
            .corner_radius(10.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if compact_toggle_button(ui, self.settings.ultra_compact).clicked() {
                        self.set_ultra_compact(ctx, false);
                    }
                    ui.label(RichText::new("今日浮盈").color(Color32::from_gray(150)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format_money(today_pnl))
                                .size(18.0 * self.settings.font_scale)
                                .strong()
                                .color(self.pnl_color(today_pnl)),
                        );
                    });
                });
            });
    }

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(Color32::from_rgb(18, 22, 29))
            .corner_radius(8.0)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("刷新").clicked() {
                        self.start_fetch();
                    }
                    if ui.small_button("添加").clicked() {
                        self.add_empty_holding();
                    }
                    if ui.selectable_label(self.show_ocr_panel, "OCR").clicked() {
                        self.show_ocr_panel = !self.show_ocr_panel;
                    }
                    if ui
                        .selectable_label(self.editing, if self.editing { "完成" } else { "编辑" })
                        .clicked()
                    {
                        if self.editing {
                            self.editing = false;
                            self.save_all();
                        } else {
                            self.editing = true;
                        }
                    }
                    if ui.small_button("保存").clicked() {
                        self.save_all();
                    }

                    ui.separator();
                    ui.label("刷新");
                    let mut refresh = self.settings.refresh_interval_secs as i32;
                    if ui
                        .add(
                            egui::DragValue::new(&mut refresh)
                                .speed(1)
                                .range(5..=300)
                                .suffix("s"),
                        )
                        .changed()
                    {
                        self.settings.refresh_interval_secs = refresh.max(5) as u64;
                        self.next_refresh_at = Instant::now()
                            + Duration::from_secs(self.settings.refresh_interval_secs);
                    }
                    ui.checkbox(&mut self.settings.always_on_top, "置顶");
                    ui.separator();
                    self.render_font_control(ui);
                    ui.separator();
                    if ui.selectable_label(self.show_ocr_panel, "AI设置").clicked() {
                        self.show_ocr_panel = !self.show_ocr_panel;
                    }
                });
            });
    }

    fn render_toolbar_toggle(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = if self.show_toolbar {
                "收起工具"
            } else {
                "工具"
            };
            if ui.small_button(label).clicked() {
                self.show_toolbar = !self.show_toolbar;
            }
            ui.label(
                RichText::new(&self.status)
                    .small()
                    .color(Color32::from_gray(150)),
            );
        });
    }

    fn render_font_control(&mut self, ui: &mut egui::Ui) {
        ui.label("字号");
        if ui.small_button("-").clicked() {
            self.settings.font_scale = (self.settings.font_scale - 0.05).max(0.8);
        }
        let slider = egui::Slider::new(&mut self.settings.font_scale, 0.8..=1.35)
            .show_value(false)
            .clamping(egui::SliderClamping::Always);
        ui.add_sized([100.0, 18.0], slider);
        if ui.small_button("+").clicked() {
            self.settings.font_scale = (self.settings.font_scale + 0.05).min(1.35);
        }
        ui.label(
            RichText::new(format!("{:.0}%", self.settings.font_scale * 100.0))
                .monospace()
                .color(Color32::from_gray(190)),
        );
    }

    fn render_ocr_panel(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(Color32::from_rgb(18, 22, 29))
            .stroke(Stroke::new(1.0, Color32::from_rgb(49, 58, 72)))
            .corner_radius(8.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("截图 OCR").strong().color(Color32::WHITE));
                        ui.label(
                            RichText::new("复制持仓截图后粘贴，或把图片拖进下面的框。")
                                .color(Color32::from_gray(165)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("关闭").clicked() {
                            self.show_ocr_panel = false;
                        }
                        if ui.button("选择图片").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("截图", &["png", "jpg", "jpeg", "bmp"])
                                .pick_file()
                            {
                                self.import_ai_ocr_path(path);
                            }
                        }
                        if ui.button("本地OCR").clicked() {
                            self.import_ocr();
                        }
                        if ui.button("剪贴板?").clicked() {
                            self.check_clipboard_image();
                        }
                        let busy = self.ai_ocr_rx.is_some();
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(if busy { "识别中..." } else { "AI识别" }),
                            )
                            .clicked()
                        {
                            self.import_ai_ocr_clipboard();
                        }
                    });
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("API Key");
                    ui.add_sized(
                        [180.0, 22.0],
                        egui::TextEdit::singleline(&mut self.settings.openai_api_key)
                            .password(true)
                            .hint_text("sk-..."),
                    );
                    ui.label("Base URL");
                    ui.add_sized(
                        [210.0, 22.0],
                        egui::TextEdit::singleline(&mut self.settings.openai_base_url)
                            .hint_text("https://api.openai.com/v1"),
                    );
                    if ui.button("获取模型").clicked() {
                        self.fetch_ai_models();
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("OCR模型");
                    model_picker(
                        ui,
                        "ocr_model_picker",
                        &mut self.settings.ocr_model,
                        &self.settings.available_models,
                    );
                    if ui.button("推荐OCR模型").clicked() {
                        self.pick_recommended_ocr_model();
                    }
                    ui.label("分析模型");
                    model_picker(
                        ui,
                        "analysis_model_picker",
                        &mut self.settings.analysis_model,
                        &self.settings.available_models,
                    );
                    if ui.button("测试OCR模型").clicked() {
                        self.test_ai_model(true);
                    }
                    if ui.button("测试分析模型").clicked() {
                        self.test_ai_model(false);
                    }
                });
                ui.add_space(10.0);
                let available = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(vec2(available, 86.0), Sense::click());
                draw_ocr_drop_zone(ui, rect);
                if response.clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("截图", &["png", "jpg", "jpeg", "bmp"])
                        .pick_file()
                    {
                        self.import_ai_ocr_path(path);
                    }
                }
            });
    }

    fn fetch_ai_models(&mut self) {
        match ai::fetch_models(
            &self.settings.openai_api_key,
            &self.settings.openai_base_url,
        ) {
            Ok(models) => {
                if !models.contains(&self.settings.ocr_model) {
                    self.settings.ocr_model = models.first().cloned().unwrap_or_default();
                }
                if !models.contains(&self.settings.analysis_model) {
                    self.settings.analysis_model = self.settings.ocr_model.clone();
                }
                self.settings.available_models = models;
                self.status = format!("已获取 {} 个模型", self.settings.available_models.len());
            }
            Err(err) => self.status = format!("获取模型失败：{err:#}"),
        }
    }

    fn test_ai_model(&mut self, ocr_model: bool) {
        let model = if ocr_model {
            self.settings.ocr_model.clone()
        } else {
            self.settings.analysis_model.clone()
        };
        self.status = format!("正在测试模型 {model}...");
        match ai::test_model(
            &self.settings.openai_api_key,
            &self.settings.openai_base_url,
            &model,
        ) {
            Ok(message) => self.status = format!("模型测试成功：{message}"),
            Err(err) => self.status = format!("模型测试失败：{err:#}"),
        }
    }

    fn pick_recommended_ocr_model(&mut self) {
        let Some(model) = recommended_ocr_model(&self.settings.available_models) else {
            self.status = "没有找到明显适合 OCR 的模型，请手动选择非 codex 的视觉模型".to_owned();
            return;
        };
        self.settings.ocr_model = model.clone();
        self.status = format!("已选择推荐 OCR 模型：{model}");
    }

    fn render_holdings(&mut self, ui: &mut egui::Ui) {
        let row_height = 28.0;
        let columns = if self.editing { 12 } else { 6 };
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Grid::new("holdings_grid")
                    .num_columns(columns)
                    .spacing(vec2(13.0, 7.0))
                    .striped(true)
                    .show(ui, |ui| {
                        table_header(ui, "名称");
                        table_header(ui, "现价");
                        table_header(ui, "涨跌幅");
                        table_header(ui, "持仓");
                        table_header(ui, "今日");
                        table_header(ui, "市值");
                        if self.editing {
                            table_header(ui, "代码");
                            table_header(ui, "数量");
                            table_header(ui, "成本");
                            table_header(ui, "成本额");
                            table_header(ui, "市场");
                            table_header(ui, "");
                        }
                        ui.end_row();

                        let mut remove_idx = None;
                        for (idx, holding) in self.portfolio.holdings.iter_mut().enumerate() {
                            let quote = self.quotes.quotes.get(&holding.code).cloned();
                            if self.editing {
                                ui.add_sized(
                                    [84.0, row_height],
                                    egui::TextEdit::singleline(&mut holding.name),
                                );
                            } else {
                                let display_name = quote
                                    .as_ref()
                                    .map(|q| q.name.as_str())
                                    .filter(|name| !name.trim().is_empty())
                                    .unwrap_or(&holding.name);
                                ui.label(RichText::new(display_name).color(Color32::WHITE));
                            }

                            if let Some(q) = quote {
                                ui.label(format!("{:.3}", q.price));
                                ui.label(
                                    RichText::new(format!("{:+.2}%", q.change_percent))
                                        .color(pnl_color_for(q.change_percent)),
                                );
                                ui.label(
                                    RichText::new(format_money(q.position_pnl(holding)))
                                        .color(pnl_color_for(q.position_pnl(holding))),
                                );
                                ui.label(
                                    RichText::new(format_money(q.today_pnl(holding)))
                                        .color(pnl_color_for(q.today_pnl(holding))),
                                );
                                ui.label(
                                    RichText::new(format_money(q.market_value(holding)))
                                        .color(Color32::from_gray(170)),
                                );
                            } else {
                                for _ in 0..5 {
                                    ui.label(RichText::new("--").color(Color32::from_gray(110)));
                                }
                            }

                            if self.editing {
                                let code_response = ui.add_sized(
                                    [72.0, row_height],
                                    egui::TextEdit::singleline(&mut holding.code),
                                );
                                if code_response.changed() {
                                    holding.code = holding
                                        .code
                                        .chars()
                                        .filter(|c| c.is_ascii_digit())
                                        .take(6)
                                        .collect();
                                    if holding.code.len() == 6 {
                                        holding.market = Market::infer(&holding.code);
                                    }
                                }
                                ui.add_sized(
                                    [86.0, row_height],
                                    egui::DragValue::new(&mut holding.quantity).speed(100.0),
                                );
                                ui.add_sized(
                                    [76.0, row_height],
                                    egui::DragValue::new(&mut holding.cost_price).speed(0.1),
                                );
                                ui.label(format_money(holding.cost_price * holding.quantity));
                                egui::ComboBox::from_id_salt(format!("market_{}", idx))
                                    .selected_text(holding.market.label())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut holding.market,
                                            Market::Shanghai,
                                            "沪",
                                        );
                                        ui.selectable_value(
                                            &mut holding.market,
                                            Market::Shenzhen,
                                            "深",
                                        );
                                        ui.selectable_value(
                                            &mut holding.market,
                                            Market::Beijing,
                                            "北",
                                        );
                                    });
                                if ui.button("删除").clicked() {
                                    remove_idx = Some(idx);
                                }
                            }
                            ui.end_row();
                        }

                        if let Some(idx) = remove_idx {
                            self.portfolio.holdings.remove(idx);
                        }
                    });
            });
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = fs::read("C:\\Windows\\Fonts\\msyh.ttc") {
        fonts
            .font_data
            .insert("msyh".to_owned(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "msyh".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "msyh".to_owned());
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(54, 62, 74));
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(35, 41, 52);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 57, 70);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(58, 111, 242);
    style.spacing.button_padding = vec2(10.0, 5.0);
    ctx.set_style(style);
}

fn apply_text_scale(ctx: &egui::Context, scale: f32) {
    let scale = scale.clamp(0.8, 1.35);
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0 * scale)),
        (TextStyle::Body, FontId::proportional(13.5 * scale)),
        (TextStyle::Monospace, FontId::monospace(13.0 * scale)),
        (TextStyle::Button, FontId::proportional(13.0 * scale)),
        (TextStyle::Small, FontId::proportional(11.0 * scale)),
    ]
    .into();
    ctx.set_style(style);
}

fn pnl_color_for(value: f64) -> Color32 {
    if value.abs() < 0.005 {
        Color32::from_gray(210)
    } else if value > 0.0 {
        Color32::from_rgb(235, 82, 82)
    } else {
        Color32::from_rgb(90, 155, 255)
    }
}

fn draw_ocr_drop_zone(ui: &egui::Ui, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 8.0, Color32::from_rgb(22, 27, 35));
    ui.painter().rect_stroke(
        rect.shrink(1.0),
        8.0,
        Stroke::new(1.0, Color32::from_rgb(75, 88, 108)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "粘贴 / 拖放 / 点击选择 同花顺或东方财富持仓截图",
        FontId::proportional(14.0),
        Color32::from_gray(185),
    );
}

fn model_picker(ui: &mut egui::Ui, id: &str, selected: &mut String, models: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(if selected.is_empty() {
            "选择模型"
        } else {
            selected.as_str()
        })
        .width(150.0)
        .show_ui(ui, |ui| {
            for model in models {
                ui.selectable_value(selected, model.clone(), model);
            }
            ui.separator();
            ui.text_edit_singleline(selected);
        });
}

fn recommended_ocr_model(models: &[String]) -> Option<String> {
    let preferred_keywords = [
        "gpt-4o",
        "gpt-4.1",
        "gpt-5.4-mini",
        "gpt-5.3-mini",
        "vision",
        "gemini",
        "claude",
        "qwen-vl",
        "qwen2.5-vl",
    ];

    preferred_keywords.iter().find_map(|keyword| {
        models
            .iter()
            .find(|model| {
                let lower = model.to_ascii_lowercase();
                lower.contains(keyword) && !lower.contains("codex") && !lower.contains("embed")
            })
            .cloned()
    })
}

fn compact_toggle_button(ui: &mut egui::Ui, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 18.0), Sense::click());
    let tint = if response.hovered() || active {
        Color32::from_rgb(255, 87, 100)
    } else {
        Color32::from_rgb(210, 62, 76)
    };
    let stroke = Stroke::new(1.4, tint);
    let icon_rect = rect.shrink2(vec2(4.0, 4.0));
    let painter = ui.painter();

    if active {
        painter.rect_stroke(icon_rect, 2.0, stroke, egui::StrokeKind::Inside);
        let inner = icon_rect.translate(vec2(3.0, -3.0));
        painter.line_segment([inner.left_top(), inner.right_top()], stroke);
        painter.line_segment([inner.right_top(), inner.right_bottom()], stroke);
    } else {
        painter.rect_stroke(icon_rect, 2.0, stroke, egui::StrokeKind::Inside);
        let y = icon_rect.center().y;
        painter.line_segment(
            [
                egui::pos2(icon_rect.left() + 3.0, y),
                egui::pos2(icon_rect.right() - 3.0, y),
            ],
            stroke,
        );
    }

    response.on_hover_text("究极缩小：只显示今日浮盈")
}

fn metric_card(ui: &mut egui::Ui, label: &str, value: f64, color: Color32, scale: f32) {
    let frame = Frame::new()
        .fill(Color32::from_rgb(25, 30, 38))
        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 54, 67)))
        .corner_radius(8.0)
        .inner_margin(8.0);

    frame.show(ui, |ui| {
        ui.set_min_size(vec2(108.0, 34.0));
        ui.label(
            RichText::new(label)
                .size(10.0 * scale)
                .color(Color32::from_gray(145)),
        );
        ui.label(
            RichText::new(format_money(value))
                .size(15.5 * scale)
                .strong()
                .color(color),
        );
    });
    ui.add_space(8.0);
}

fn table_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .small()
            .strong()
            .color(Color32::from_gray(160)),
    );
}

fn format_money(value: f64) -> String {
    if value.abs() >= 10_000.0 {
        format!("{:.2}万", value / 10_000.0)
    } else {
        format!("{value:.2}")
    }
}

fn delay_until_next_market_session(now: chrono::DateTime<Local>) -> Option<Duration> {
    if is_market_session_time(now.naive_local()) {
        return None;
    }

    let next_open = next_market_open_after(now.naive_local());
    Some(
        (next_open - now.naive_local())
            .to_std()
            .unwrap_or_else(|_| Duration::from_secs(60)),
    )
}

fn is_market_session_time(now: NaiveDateTime) -> bool {
    if !is_weekday(now.weekday()) {
        return false;
    }

    let time = now.time();
    let morning_start = market_time(9, 20);
    let morning_end = market_time(11, 30);
    let afternoon_start = market_time(13, 0);
    let afternoon_end = market_time(15, 0);

    (time >= morning_start && time <= morning_end)
        || (time >= afternoon_start && time <= afternoon_end)
}

fn next_market_open_after(now: NaiveDateTime) -> NaiveDateTime {
    let morning_start = market_time(9, 20);
    let afternoon_start = market_time(13, 0);
    let morning_end = market_time(11, 30);
    let afternoon_end = market_time(15, 0);

    if is_weekday(now.weekday()) {
        let time = now.time();
        if time < morning_start {
            return now.date().and_time(morning_start);
        }
        if time > morning_end && time < afternoon_start {
            return now.date().and_time(afternoon_start);
        }
        if time <= afternoon_end {
            return now.date().and_time(afternoon_start);
        }
    }

    let mut date = now.date() + chrono::Duration::days(1);
    while !is_weekday(date.weekday()) {
        date += chrono::Duration::days(1);
    }
    date.and_time(morning_start)
}

fn is_weekday(weekday: Weekday) -> bool {
    !matches!(weekday, Weekday::Sat | Weekday::Sun)
}

fn market_time(hour: u32, minute: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(hour, minute, 0).expect("valid market time")
}

fn format_duration_for_status(duration: Duration) -> String {
    let total_minutes = (duration.as_secs() + 59) / 60;
    if total_minutes < 60 {
        format!("{total_minutes}分钟")
    } else {
        let hours = total_minutes / 60;
        let minutes = total_minutes % 60;
        if minutes == 0 {
            format!("{hours}小时")
        } else {
            format!("{hours}小时{minutes}分钟")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(date: &str, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    #[test]
    fn market_session_starts_at_call_auction_time() {
        assert!(!is_market_session_time(dt("2026-04-29", 9, 19)));
        assert!(is_market_session_time(dt("2026-04-29", 9, 20)));
        assert!(is_market_session_time(dt("2026-04-29", 11, 30)));
    }

    #[test]
    fn market_session_skips_lunch_break_and_after_close() {
        assert!(!is_market_session_time(dt("2026-04-29", 11, 31)));
        assert!(is_market_session_time(dt("2026-04-29", 13, 0)));
        assert!(is_market_session_time(dt("2026-04-29", 15, 0)));
        assert!(!is_market_session_time(dt("2026-04-29", 15, 1)));
    }

    #[test]
    fn next_open_handles_lunch_after_close_and_weekends() {
        assert_eq!(
            next_market_open_after(dt("2026-04-29", 11, 31)),
            dt("2026-04-29", 13, 0)
        );
        assert_eq!(
            next_market_open_after(dt("2026-04-29", 15, 1)),
            dt("2026-04-30", 9, 20)
        );
        assert_eq!(
            next_market_open_after(dt("2026-05-02", 10, 0)),
            dt("2026-05-04", 9, 20)
        );
    }
}
