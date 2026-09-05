use crate::{
    ai::{self, RecognizedPortfolio},
    calendar,
    config::{self, AppSettings},
    notification, ocr,
    portfolio::{
        Account, AlertKind, AlertRule, DailySnapshot, Holding, Market, Portfolio, Transaction,
        TransactionKind, WatchItem, DEFAULT_OWNER,
    },
    quote::{self, QuoteBook, QuoteFetchResult},
    updater::{self, UpdateInfo},
};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime};
use eframe::egui::{
    self, vec2, Align, Color32, FontData, FontFamily, FontId, Frame, Grid, Key, Layout, Rect,
    RichText, ScrollArea, Sense, Stroke, TextStyle, ViewportCommand,
};
use image::{ImageBuffer, Rgba};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::PathBuf,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

const NORMAL_WINDOW_SIZE: [f32; 2] = [720.0, 420.0];
const COMPACT_WINDOW_SIZE: [f32; 2] = [252.0, 112.0];
const COMBINED_OWNER: &str = "C";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemePreset {
    Classic,
    Copper,
    Slate,
    Carbon,
    Nord,
}

impl ThemePreset {
    const ALL: [Self; 5] = [
        Self::Classic,
        Self::Copper,
        Self::Slate,
        Self::Carbon,
        Self::Nord,
    ];

    fn from_id(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "classic" => Self::Classic,
            "slate" => Self::Slate,
            "carbon" => Self::Carbon,
            "nord" => Self::Nord,
            _ => Self::Copper,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Copper => "copper",
            Self::Slate => "slate",
            Self::Carbon => "carbon",
            Self::Nord => "nord",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Classic => "经典暗黑",
            Self::Copper => "赤铜",
            Self::Slate => "石板",
            Self::Carbon => "极黑",
            Self::Nord => "冰夜",
        }
    }

    fn palette(self) -> ThemePalette {
        match self {
            Self::Classic => ThemePalette {
                background: rgb(14, 17, 22),
                surface: rgb(19, 23, 30),
                surface_alt: rgb(25, 30, 38),
                border: rgb(45, 54, 67),
                card_border: rgb(45, 54, 67),
                badge: rgb(38, 40, 44),
                text_primary: gray(235),
                text_secondary: gray(174),
                text_muted: gray(135),
                accent: rgb(58, 111, 242),
                gain: rgb(235, 82, 82),
                loss: rgb(90, 155, 255),
                flat: gray(210),
                warning: rgb(245, 180, 65),
                radius: 8.0,
            },
            Self::Copper => ThemePalette {
                background: rgb(17, 18, 20),
                surface: rgb(25, 26, 29),
                surface_alt: rgb(33, 34, 38),
                border: rgb(50, 52, 58),
                card_border: Color32::TRANSPARENT,
                badge: rgb(33, 34, 38),
                text_primary: rgb(238, 235, 230),
                text_secondary: rgb(180, 174, 165),
                text_muted: rgb(137, 132, 124),
                accent: rgb(201, 130, 69),
                gain: rgb(240, 90, 90),
                loss: rgb(77, 159, 184),
                flat: rgb(205, 201, 195),
                warning: rgb(230, 161, 92),
                radius: 3.0,
            },
            Self::Slate => ThemePalette {
                background: rgb(17, 22, 30),
                surface: rgb(23, 30, 40),
                surface_alt: rgb(32, 42, 54),
                border: rgb(48, 62, 76),
                card_border: Color32::TRANSPARENT,
                badge: rgb(32, 42, 54),
                text_primary: rgb(235, 239, 244),
                text_secondary: rgb(177, 187, 198),
                text_muted: rgb(126, 139, 153),
                accent: rgb(79, 155, 168),
                gain: rgb(240, 90, 90),
                loss: rgb(87, 148, 216),
                flat: rgb(212, 219, 226),
                warning: rgb(230, 161, 92),
                radius: 6.0,
            },
            Self::Carbon => ThemePalette {
                background: rgb(22, 22, 22),
                surface: rgb(38, 38, 38),
                surface_alt: rgb(57, 57, 57),
                border: rgb(70, 70, 70),
                card_border: Color32::TRANSPARENT,
                badge: rgb(38, 38, 38),
                text_primary: rgb(244, 244, 244),
                text_secondary: rgb(198, 198, 198),
                text_muted: rgb(150, 150, 150),
                accent: rgb(15, 98, 254),
                gain: rgb(250, 77, 86),
                loss: rgb(66, 190, 206),
                flat: rgb(218, 218, 218),
                warning: rgb(255, 131, 43),
                radius: 1.0,
            },
            Self::Nord => ThemePalette {
                background: rgb(46, 52, 64),
                surface: rgb(59, 66, 82),
                surface_alt: rgb(67, 76, 94),
                border: rgb(76, 86, 106),
                card_border: rgb(76, 86, 106),
                badge: rgb(59, 66, 82),
                text_primary: rgb(236, 239, 244),
                text_secondary: rgb(216, 222, 233),
                text_muted: rgb(143, 155, 174),
                accent: rgb(136, 192, 208),
                gain: rgb(191, 97, 106),
                loss: rgb(94, 129, 172),
                flat: rgb(229, 233, 240),
                warning: rgb(235, 203, 139),
                radius: 4.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ThemePalette {
    background: Color32,
    surface: Color32,
    surface_alt: Color32,
    border: Color32,
    card_border: Color32,
    badge: Color32,
    text_primary: Color32,
    text_secondary: Color32,
    text_muted: Color32,
    accent: Color32,
    gain: Color32,
    loss: Color32,
    flat: Color32,
    warning: Color32,
    radius: f32,
}

const fn rgb(red: u8, green: u8, blue: u8) -> Color32 {
    Color32::from_rgb(red, green, blue)
}

const fn gray(value: u8) -> Color32 {
    Color32::from_gray(value)
}

#[derive(Debug, Clone, Copy, Default)]
struct PortfolioTotals {
    total_assets: f64,
    market_value: f64,
    position_pnl: f64,
    today_pnl: f64,
}

impl PortfolioTotals {
    fn position_percent(self) -> Option<f64> {
        (self.total_assets > 0.005).then_some(self.market_value / self.total_assets * 100.0)
    }

    fn today_pnl_percent(self) -> Option<f64> {
        let previous_assets = self.total_assets - self.today_pnl;
        (previous_assets.abs() > 0.005).then_some(self.today_pnl / previous_assets * 100.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPanel {
    Ocr,
    Ledger,
    Alerts,
    Risk,
    Watchlist,
    Backups,
    Detail,
}

#[derive(Debug, Clone)]
struct TransactionDraft {
    account_index: usize,
    kind: TransactionKind,
    date: String,
    code: String,
    name: String,
    quantity: f64,
    price: f64,
    fees: f64,
    cash_amount: f64,
    note: String,
}

impl Default for TransactionDraft {
    fn default() -> Self {
        Self {
            account_index: 0,
            kind: TransactionKind::Buy,
            date: Local::now().format("%Y-%m-%d").to_string(),
            code: String::new(),
            name: String::new(),
            quantity: 100.0,
            price: 0.0,
            fees: 0.0,
            cash_amount: 0.0,
            note: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct AlertDraft {
    code: String,
    name: String,
    kind: AlertKind,
    threshold: f64,
}

impl Default for AlertDraft {
    fn default() -> Self {
        Self {
            code: String::new(),
            name: String::new(),
            kind: AlertKind::PriceAbove,
            threshold: 0.0,
        }
    }
}

pub struct StockWatchApp {
    portfolio: Portfolio,
    settings: AppSettings,
    quotes: QuoteBook,
    fetch_rx: Option<Receiver<anyhow::Result<QuoteFetchResult>>>,
    ai_ocr_rx: Option<Receiver<anyhow::Result<RecognizedPortfolio>>>,
    update_rx: Option<Receiver<anyhow::Result<UpdateInfo>>>,
    update_info: Option<UpdateInfo>,
    next_refresh_at: Instant,
    quote_failure_count: u32,
    status: String,
    editing: bool,
    active_panel: Option<ToolPanel>,
    show_toolbar: bool,
    pending_import: Option<RecognizedPortfolio>,
    import_account_index: usize,
    import_replace_missing: bool,
    transaction_draft: TransactionDraft,
    alert_draft: AlertDraft,
    watch_code: String,
    watch_name: String,
    selected_detail_code: Option<String>,
    last_snapshot_save_at: Option<Instant>,
    pending_attention: bool,
    selected_owner: String,
}

impl StockWatchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        setup_fonts(&cc.egui_ctx);
        let mut portfolio = config::load_portfolio()?;
        portfolio.normalize();
        let settings = config::load_settings();
        let cached_quotes = quote::load_cache();
        let mut app = Self {
            portfolio,
            settings,
            quotes: cached_quotes,
            fetch_rx: None,
            ai_ocr_rx: None,
            update_rx: Some(updater::spawn_check()),
            update_info: None,
            next_refresh_at: Instant::now(),
            quote_failure_count: 0,
            status: "准备刷新行情".to_owned(),
            editing: false,
            active_panel: None,
            show_toolbar: false,
            pending_import: None,
            import_account_index: 0,
            import_replace_missing: false,
            transaction_draft: TransactionDraft::default(),
            alert_draft: AlertDraft::default(),
            watch_code: String::new(),
            watch_name: String::new(),
            selected_detail_code: None,
            last_snapshot_save_at: None,
            pending_attention: false,
            selected_owner: DEFAULT_OWNER.to_owned(),
        };
        if app.settings.ultra_compact {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                    COMPACT_WINDOW_SIZE[0],
                    COMPACT_WINDOW_SIZE[1],
                )));
        }
        app.start_fetch(true);
        Ok(app)
    }

    fn start_fetch(&mut self, force_once: bool) {
        if self.fetch_rx.is_some() {
            return;
        }
        let off_session_delay = delay_until_next_market_session(Local::now());
        if !force_once {
            if let Some(delay) = off_session_delay {
                self.quotes.loading = false;
                self.next_refresh_at = Instant::now() + delay;
                self.status = format!("非交易时段，{} 后刷新", format_duration_for_status(delay));
                return;
            }
        }
        self.portfolio.normalize();
        self.quotes.loading = true;
        self.fetch_rx = Some(quote::spawn_fetch(self.portfolio.quote_targets()));
        self.status = if off_session_delay.is_some() {
            "正在拉取非交易时段行情...".to_owned()
        } else {
            "正在刷新行情...".to_owned()
        };
    }

    fn poll_fetch(&mut self) {
        let Some(rx) = &self.fetch_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                let updated_at = result
                    .quotes
                    .iter()
                    .map(|quote| quote.updated_at)
                    .min()
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
                let _ = quote::save_cache(&self.quotes.quotes);
                self.evaluate_alerts();
                self.record_snapshot_if_due();
                if let Some(delay) = delay_until_next_market_session(Local::now()) {
                    self.next_refresh_at = Instant::now() + delay;
                    self.status = format!(
                        "行情已更新（{}，非交易时段）；{} 后刷新",
                        result.source.label(),
                        format_duration_for_status(delay)
                    );
                } else {
                    self.next_refresh_at = Instant::now() + self.normal_refresh_delay();
                    self.status = format!("行情已更新（{}）", result.source.label());
                }
            }
            Ok(Err(err)) => {
                self.quotes.last_error = Some(err.to_string());
                self.quotes.loading = false;
                self.fetch_rx = None;
                self.quote_failure_count = self.quote_failure_count.saturating_add(1);
                let delay = delay_until_next_market_session(Local::now())
                    .unwrap_or_else(|| self.failure_refresh_delay());
                self.next_refresh_at = Instant::now() + delay;
                self.status = format!(
                    "行情刷新失败：{err:#}；{} 后重试",
                    format_duration_for_status(delay)
                );
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
            Ok(Ok(result)) if result.holdings.is_empty() => {
                self.ai_ocr_rx = None;
                self.status = "AI 没有从截图中识别出可用持仓行".to_owned();
            }
            Ok(Ok(result)) => {
                self.ai_ocr_rx = None;
                self.import_account_index = self.best_import_account(&result);
                self.pending_import = Some(result);
                self.active_panel = Some(ToolPanel::Ocr);
                self.status = "AI 识别完成，请确认持仓差异后再应用".to_owned();
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

    fn poll_update_check(&mut self) {
        let Some(rx) = &self.update_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(info)) => {
                if info.newer {
                    self.status = format!("发现新版本 {}", info.tag);
                }
                self.update_info = Some(info);
                self.update_rx = None;
            }
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.update_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
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
        if is_combined_owner(&self.selected_owner) {
            self.status = "合并分区为只读，请切换到真实分区后添加".to_owned();
            return;
        }
        let owner = self.selected_owner.trim().to_owned();
        let owner = if owner.is_empty() {
            DEFAULT_OWNER.to_owned()
        } else {
            owner
        };
        let account_index = self
            .portfolio
            .accounts
            .iter()
            .position(|account| account_matches_owner(account, &owner))
            .unwrap_or_else(|| {
                self.portfolio.accounts.push(Account {
                    id: format!("account-{}", self.portfolio.accounts.len() + 1),
                    name: owner.clone(),
                    owner: owner.clone(),
                    account_suffix: String::new(),
                    cash: 0.0,
                    today_realized_pnl: 0.0,
                    today_realized_pnl_date: None,
                    holdings: Vec::new(),
                });
                self.portfolio.accounts.len() - 1
            });
        self.portfolio.accounts[account_index]
            .holdings
            .push(Holding {
                code: String::new(),
                name: String::new(),
                quantity: 0.0,
                available_quantity: None,
                available_date: None,
                intraday_cost_price: None,
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
            Ok(holdings) => {
                self.pending_import = Some(RecognizedPortfolio {
                    holdings,
                    as_of_date: Some(Local::now().date_naive()),
                    ..Default::default()
                });
                self.import_account_index = 0;
                self.active_panel = Some(ToolPanel::Ocr);
                self.status = "本地 OCR 完成，请确认持仓差异后再应用".to_owned();
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

    fn best_import_account(&self, result: &RecognizedPortfolio) -> usize {
        let hints = [result.broker.as_deref(), result.account_name.as_deref()];
        self.portfolio
            .accounts
            .iter()
            .enumerate()
            .find(|(_, account)| {
                let suffix = account.account_suffix.trim();
                hints.iter().flatten().any(|hint| {
                    (!suffix.is_empty() && hint.contains(suffix))
                        || account.name.contains(*hint)
                        || hint.contains(&account.name)
                })
            })
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn apply_pending_import(&mut self) {
        let Some(result) = self.pending_import.clone() else {
            return;
        };
        let Some(account) = self.portfolio.accounts.get(self.import_account_index) else {
            self.status = "请选择有效账户".to_owned();
            return;
        };
        let account_id = account.id.clone();
        let date = result
            .as_of_date
            .unwrap_or_else(|| Local::now().date_naive());
        let cash = result.cash.or_else(|| {
            result
                .total_assets
                .zip(result.market_value)
                .map(|(total, market)| total - market)
        });
        match self.portfolio.reconcile_account(
            &account_id,
            &result.holdings,
            cash,
            self.import_replace_missing,
            date,
        ) {
            Ok(summary) => match config::save_portfolio(&mut self.portfolio) {
                Ok(_) => {
                    self.pending_import = None;
                    self.status = format!(
                        "持仓差异已应用：新增 {}、修改 {}、清除 {}",
                        summary.added, summary.changed, summary.removed
                    );
                    self.start_fetch(true);
                }
                Err(err) => self.status = format!("应用成功但保存失败：{err:#}"),
            },
            Err(err) => self.status = format!("应用持仓差异失败：{err:#}"),
        }
    }

    fn submit_transaction(&mut self) {
        let Some(account) = self
            .portfolio
            .accounts
            .get(self.transaction_draft.account_index)
        else {
            self.status = "请选择有效账户".to_owned();
            return;
        };
        let Ok(date) = NaiveDate::parse_from_str(&self.transaction_draft.date, "%Y-%m-%d") else {
            self.status = "交易日期格式应为 YYYY-MM-DD".to_owned();
            return;
        };
        let code = self
            .transaction_draft
            .code
            .chars()
            .filter(|character| character.is_ascii_digit())
            .take(6)
            .collect::<String>();
        let transaction = Transaction {
            id: String::new(),
            account_id: account.id.clone(),
            date,
            kind: self.transaction_draft.kind,
            market: Market::infer(&code),
            code,
            name: self.transaction_draft.name.clone(),
            quantity: self.transaction_draft.quantity,
            price: self.transaction_draft.price,
            fees: self.transaction_draft.fees,
            cash_amount: self.transaction_draft.cash_amount,
            realized_pnl: None,
            execution: Default::default(),
            note: self.transaction_draft.note.clone(),
        };
        match self.portfolio.apply_transaction(transaction) {
            Ok(_) => match config::save_portfolio(&mut self.portfolio) {
                Ok(_) => {
                    self.status = format!("已记录{}", self.transaction_draft.kind.label());
                    self.transaction_draft.code.clear();
                    self.transaction_draft.name.clear();
                    self.transaction_draft.quantity = 100.0;
                    self.transaction_draft.price = 0.0;
                    self.transaction_draft.fees = 0.0;
                    self.transaction_draft.cash_amount = 0.0;
                    self.transaction_draft.note.clear();
                    self.start_fetch(true);
                }
                Err(err) => self.status = format!("交易已应用但保存失败：{err:#}"),
            },
            Err(err) => self.status = format!("记录交易失败：{err:#}"),
        }
    }

    fn evaluate_alerts(&mut self) {
        let mut messages = Vec::new();
        let mut changed = false;
        for alert in &mut self.portfolio.alerts {
            if !alert.enabled {
                continue;
            }
            let Some(quote) = self.quotes.quotes.get(&alert.code) else {
                continue;
            };
            if quote.price > alert.high_watermark {
                alert.high_watermark = quote.price;
                changed = true;
            }
            let today_pnl = self
                .portfolio
                .accounts
                .iter()
                .flat_map(|account| account.holdings.iter())
                .filter(|holding| holding.code == alert.code)
                .map(|holding| quote.today_pnl(holding))
                .sum::<f64>();
            let drawdown = if alert.high_watermark > 0.0 {
                (alert.high_watermark - quote.price) / alert.high_watermark * 100.0
            } else {
                0.0
            };
            let matching = match alert.kind {
                AlertKind::PriceAbove => quote.price >= alert.threshold,
                AlertKind::PriceBelow => quote.price <= alert.threshold,
                AlertKind::ChangeAbove => quote.change_percent >= alert.threshold,
                AlertKind::ChangeBelow => quote.change_percent <= alert.threshold,
                AlertKind::TodayPnlBelow => today_pnl <= -alert.threshold.abs(),
                AlertKind::DrawdownFromHigh => drawdown >= alert.threshold.abs(),
            };
            if matching && !alert.was_matching {
                let display_name = if alert.name.trim().is_empty() {
                    &quote.name
                } else {
                    &alert.name
                };
                messages.push(format!(
                    "{} {}触发：现价 {:.2}，涨跌 {:+.2}%",
                    display_name,
                    alert.kind.label(),
                    quote.price,
                    quote.change_percent
                ));
                alert.last_triggered_at = Some(Local::now());
                self.pending_attention = true;
            }
            if alert.was_matching != matching {
                alert.was_matching = matching;
                changed = true;
            }
        }
        if let Some(message) = messages.first() {
            notification::show("持仓提醒", message);
            self.status = message.clone();
        }
        if changed {
            let _ = config::save_portfolio_quiet(&mut self.portfolio);
        }
    }

    fn record_snapshot_if_due(&mut self) {
        if self
            .last_snapshot_save_at
            .is_some_and(|last| last.elapsed() < Duration::from_secs(300))
        {
            return;
        }
        let totals = self.totals_for_owner(DEFAULT_OWNER);
        if self.portfolio.holdings().any(|h| {
            self.quotes.quotes.get(&h.code).is_none_or(|q| {
                q.updated_at.date_naive() != Local::now().date_naive()
                    || (Local::now() - q.updated_at).num_seconds() > 120
            })
        }) {
            return;
        }
        if totals.total_assets <= 0.0
            || !totals.total_assets.is_finite()
            || !totals.today_pnl.is_finite()
            || !totals.position_pnl.is_finite()
        {
            return;
        }
        let date = self
            .quotes
            .last_updated_at
            .map(|value| value.date_naive())
            .unwrap_or_else(|| Local::now().date_naive());
        let benchmark = self.quotes.quotes.get("000300");
        self.portfolio.upsert_snapshot(DailySnapshot {
            date,
            total_assets: totals.total_assets,
            market_value: totals.market_value,
            cash: self.cash_for_owner(DEFAULT_OWNER),
            position_pnl: totals.position_pnl,
            today_pnl: totals.today_pnl,
            benchmark_level: benchmark.map(|quote| quote.price),
            benchmark_change_percent: benchmark.map(|quote| quote.change_percent),
        });
        if config::save_portfolio_quiet(&mut self.portfolio).is_ok() {
            self.last_snapshot_save_at = Some(Instant::now());
        }
    }

    fn totals(&self) -> PortfolioTotals {
        self.totals_for_owner(&self.selected_owner)
    }

    fn totals_for_owner(&self, owner: &str) -> PortfolioTotals {
        let accounts = self
            .portfolio
            .accounts
            .iter()
            .filter(|account| account_matches_owner(account, owner))
            .collect::<Vec<_>>();
        let (market_value, position_pnl, today_pnl) = accounts
            .iter()
            .flat_map(|account| account.holdings.iter())
            .fold((0.0, 0.0, 0.0), |acc, h| {
                if let Some(q) = self.quotes.quotes.get(&h.code) {
                    (
                        acc.0 + q.market_value(h),
                        acc.1 + q.position_pnl(h),
                        acc.2 + q.today_pnl(h),
                    )
                } else {
                    (f64::NAN, f64::NAN, f64::NAN)
                }
            });
        let cash = self.cash_for_owner(owner);
        let quote_date = self
            .quotes
            .last_updated_at
            .map(|updated_at| updated_at.date_naive())
            .unwrap_or_else(|| Local::now().date_naive());
        let account_ids = accounts
            .iter()
            .map(|account| account.id.as_str())
            .collect::<HashSet<_>>();
        let manual_realized = accounts
            .iter()
            .map(|account| account.today_realized_pnl_for(quote_date))
            .filter(|pnl| pnl.is_finite())
            .sum::<f64>();
        let ledger_daily = self
            .portfolio
            .transactions
            .iter()
            .filter(|transaction| {
                transaction.date == quote_date
                    && account_ids.contains(transaction.account_id.as_str())
                    && matches!(
                        transaction.kind,
                        TransactionKind::Sell | TransactionKind::Dividend | TransactionKind::Fee
                    )
            })
            .map(|transaction| {
                transaction.daily_pnl(
                    self.quotes
                        .quotes
                        .get(&transaction.code)
                        .map(|q| q.previous_close),
                )
            })
            .try_fold(0.0, |sum, item| item.map(|value| sum + value));
        PortfolioTotals {
            total_assets: market_value + cash,
            market_value,
            position_pnl,
            today_pnl: today_pnl + manual_realized + ledger_daily.unwrap_or(f64::NAN),
        }
    }

    fn cash_for_owner(&self, owner: &str) -> f64 {
        self.portfolio
            .accounts
            .iter()
            .filter(|account| account_matches_owner(account, owner))
            .map(|account| account.cash)
            .filter(|cash| cash.is_finite())
            .sum()
    }

    fn owner_options(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut options = Vec::new();
        for label in [DEFAULT_OWNER, "A", "B", COMBINED_OWNER] {
            if seen.insert(label.to_owned()) {
                options.push(label.to_owned());
            }
        }
        for account in &self.portfolio.accounts {
            let owner = account.owner_label().to_owned();
            if seen.insert(owner.clone()) {
                options.push(owner);
            }
        }
        options
    }

    fn ensure_selected_owner(&mut self) {
        if self.selected_owner.trim().is_empty() {
            self.selected_owner = DEFAULT_OWNER.to_owned();
        }
        if !self
            .owner_options()
            .iter()
            .any(|owner| owner == &self.selected_owner)
        {
            self.selected_owner = DEFAULT_OWNER.to_owned();
        }
    }

    fn pnl_color(&self, value: f64) -> Color32 {
        pnl_color_for(value, self.palette())
    }

    fn theme(&self) -> ThemePreset {
        ThemePreset::from_id(&self.settings.theme)
    }

    fn palette(&self) -> ThemePalette {
        self.theme().palette()
    }

    fn set_ultra_compact(&mut self, ctx: &egui::Context, compact: bool) {
        if compact {
            if let Some(size) = ctx.input(|i| i.viewport().inner_rect.map(|rect| rect.size())) {
                if size.x > COMPACT_WINDOW_SIZE[0] + 24.0 || size.y > COMPACT_WINDOW_SIZE[1] + 24.0
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
            let size = self
                .settings
                .normal_window_size
                .unwrap_or(NORMAL_WINDOW_SIZE);
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(size[0], size[1])));
        }
        let _ = config::save_settings(&self.settings);
    }
}

impl eframe::App for StockWatchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_fetch();
        self.poll_ai_ocr();
        self.poll_update_check();
        if Instant::now() >= self.next_refresh_at && !self.editing {
            self.start_fetch(false);
        }
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
            if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));
        if self.pending_attention {
            ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            self.pending_attention = false;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let palette = self.palette();
        apply_theme(ctx, self.theme(), self.settings.font_scale);
        self.handle_dropped_files(ctx);
        self.handle_ocr_shortcuts(ctx);

        let panel_margin = if self.settings.ultra_compact {
            6.0
        } else {
            12.0
        };
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(palette.background)
                    .inner_margin(panel_margin),
            )
            .show(ctx, |ui| {
                if self.settings.ultra_compact {
                    self.render_ultra_compact(ui, ctx);
                    return;
                }

                self.ensure_selected_owner();
                self.render_header(ui, ctx);
                ui.add_space(8.0);
                self.render_toolbar_toggle(ui);
                if self.show_toolbar {
                    ui.add_space(6.0);
                    self.render_toolbar(ui);
                }
                if self.active_panel.is_some() {
                    ui.add_space(8.0);
                    self.render_active_panel(ui);
                }
                ui.add_space(8.0);
                self.render_holdings(ui);
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&self.status)
                                .color(palette.text_secondary)
                                .small(),
                        );
                    });
                });
            });
    }
}

impl StockWatchApp {
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if self.active_panel != Some(ToolPanel::Ocr) {
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
        if self.active_panel != Some(ToolPanel::Ocr) {
            return;
        }

        let paste_pressed = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::V));
        if paste_pressed {
            self.status = "收到 Ctrl+V".to_owned();
            self.import_ai_ocr_clipboard();
        }
    }

    fn render_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let totals = self.totals();
        let palette = self.palette();
        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(palette.radius + 2.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let time = Local::now().format("%H:%M:%S").to_string();
                    ui.label(
                        RichText::new(format!("A股 · {time}"))
                            .color(palette.text_secondary)
                            .strong(),
                    );
                    if let Some(updated_at) = self.quotes.last_updated_at {
                        let age = (Local::now() - updated_at).num_seconds().max(0);
                        let stale = age > 120;
                        ui.label(
                            RichText::new(if stale {
                                format!("行情已过期 {}分", age / 60)
                            } else {
                                format!("行情 {}秒前", age)
                            })
                            .small()
                            .color(if stale {
                                palette.warning
                            } else {
                                palette.text_muted
                            }),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        position_badge(
                            ui,
                            totals.position_percent(),
                            self.settings.font_scale,
                            palette,
                        );
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("今日").color(palette.text_muted));
                            ui.label(
                                RichText::new(format_money(totals.today_pnl))
                                    .monospace()
                                    .color(self.pnl_color(totals.today_pnl))
                                    .strong(),
                            );
                        });
                        if compact_toggle_button(ui, self.settings.ultra_compact).clicked() {
                            self.set_ultra_compact(ctx, true);
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    metric_card(
                        ui,
                        "总资产",
                        totals.total_assets,
                        palette.text_primary,
                        self.settings.font_scale,
                        None,
                        palette,
                    );
                    metric_card(
                        ui,
                        "持仓盈亏",
                        totals.position_pnl,
                        self.pnl_color(totals.position_pnl),
                        self.settings.font_scale,
                        None,
                        palette,
                    );
                    metric_card(
                        ui,
                        "今日盈亏",
                        totals.today_pnl,
                        self.pnl_color(totals.today_pnl),
                        self.settings.font_scale,
                        totals.today_pnl_percent(),
                        palette,
                    );
                });
            });
    }

    fn render_ultra_compact(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let totals = self.totals();
        let palette = self.palette();
        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(palette.radius + 2.0)
            .inner_margin(7.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if compact_toggle_button(ui, self.settings.ultra_compact).clicked() {
                        self.set_ultra_compact(ctx, false);
                    }
                    ui.label(RichText::new("今日").small().color(palette.text_muted));
                    ui.label(
                        RichText::new(format_money(totals.today_pnl))
                            .monospace()
                            .size(16.5 * self.settings.font_scale)
                            .strong()
                            .color(self.pnl_color(totals.today_pnl)),
                    );
                    today_pnl_percent_label(ui, totals, self.settings.font_scale * 0.95, palette);
                });
            });
    }

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        Frame::new()
            .fill(palette.surface)
            .corner_radius(palette.radius)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("刷新").clicked() {
                        self.start_fetch(true);
                    }
                    let editable_owner = !is_combined_owner(&self.selected_owner);
                    if ui
                        .add_enabled(editable_owner, egui::Button::new("添加").small())
                        .on_hover_text(if editable_owner {
                            "添加持仓"
                        } else {
                            "合并分区为只读，请切换到真实分区后添加"
                        })
                        .clicked()
                    {
                        self.add_empty_holding();
                    }
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Ledger, "交易");
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Ocr, "OCR");
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Alerts, "提醒");
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Risk, "分析");
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Watchlist, "自选");
                    panel_toggle(ui, &mut self.active_panel, ToolPanel::Backups, "恢复");
                    if ui
                        .add_enabled(
                            editable_owner,
                            egui::Button::new(if self.editing { "完成" } else { "编辑" })
                                .selected(self.editing)
                                .small(),
                        )
                        .on_hover_text(if editable_owner {
                            "编辑当前分区"
                        } else {
                            "合并分区为只读"
                        })
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
                    if let Some(update) = &self.update_info {
                        if update.newer && ui.small_button(format!("更新 {}", update.tag)).clicked()
                        {
                            self.status = format!("正在打开 {} {}", update.name, update.tag);
                            updater::open_release(&update.url);
                        }
                    }
                });
            });
    }

    fn render_toolbar_toggle(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        ui.horizontal(|ui| {
            let label = if self.show_toolbar {
                "收起工具"
            } else {
                "工具"
            };
            if ui.small_button(label).clicked() {
                self.show_toolbar = !self.show_toolbar;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.horizontal(|ui| self.render_owner_switch(ui));
                ui.add_space(4.0);
                self.render_theme_switch(ui);
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&self.status)
                            .small()
                            .color(palette.text_muted),
                    )
                    .truncate(),
                )
                .on_hover_text(&self.status);
            });
        });
    }

    fn render_theme_switch(&mut self, ui: &mut egui::Ui) {
        let current = self.theme();
        let mut selected = current;
        egui::ComboBox::from_id_salt("theme_switch")
            .selected_text(current.label())
            .width(58.0)
            .show_ui(ui, |ui| {
                for theme in ThemePreset::ALL {
                    ui.selectable_value(&mut selected, theme, theme.label());
                }
            })
            .response
            .on_hover_text("切换界面主题");
        if selected != current {
            self.settings.theme = selected.id().to_owned();
            self.status = match config::save_settings(&self.settings) {
                Ok(_) => format!("已切换主题：{}", selected.label()),
                Err(error) => format!("主题已切换，但保存失败：{error:#}"),
            };
        }
    }

    fn render_owner_switch(&mut self, ui: &mut egui::Ui) {
        let options = self.owner_options();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for owner in options.into_iter().rev() {
                let selected = self.selected_owner == owner;
                if ui
                    .add(
                        egui::Button::new(RichText::new(&owner).small())
                            .selected(selected)
                            .min_size(vec2(28.0, 18.0)),
                    )
                    .on_hover_text("切换资金视图")
                    .clicked()
                {
                    if is_combined_owner(&owner) && self.editing {
                        self.editing = false;
                        self.save_all();
                    }
                    self.selected_owner = owner;
                }
            }
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
                .color(self.palette().text_secondary),
        );
    }

    fn render_active_panel(&mut self, ui: &mut egui::Ui) {
        match self.active_panel {
            Some(ToolPanel::Ocr) => self.render_ocr_panel(ui),
            Some(ToolPanel::Ledger) => self.render_ledger_panel(ui),
            Some(ToolPanel::Alerts) => self.render_alerts_panel(ui),
            Some(ToolPanel::Risk) => self.render_risk_panel(ui),
            Some(ToolPanel::Watchlist) => self.render_watchlist_panel(ui),
            Some(ToolPanel::Backups) => self.render_backups_panel(ui),
            Some(ToolPanel::Detail) => self.render_detail_panel(ui),
            None => {}
        }
    }

    fn render_ledger_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "交易流水", &mut self.active_panel);
            ui.horizontal_wrapped(|ui| {
                account_picker(
                    ui,
                    "transaction_account",
                    &self.portfolio,
                    &mut self.transaction_draft.account_index,
                );
                egui::ComboBox::from_id_salt("transaction_kind")
                    .selected_text(self.transaction_draft.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in [
                            TransactionKind::Buy,
                            TransactionKind::Sell,
                            TransactionKind::Dividend,
                            TransactionKind::Fee,
                            TransactionKind::Deposit,
                            TransactionKind::Withdraw,
                        ] {
                            ui.selectable_value(
                                &mut self.transaction_draft.kind,
                                kind,
                                kind.label(),
                            );
                        }
                    });
                ui.add_sized(
                    [94.0, 22.0],
                    egui::TextEdit::singleline(&mut self.transaction_draft.date),
                );
                if matches!(
                    self.transaction_draft.kind,
                    TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend
                ) {
                    ui.label("代码");
                    ui.add_sized(
                        [66.0, 22.0],
                        egui::TextEdit::singleline(&mut self.transaction_draft.code),
                    );
                    ui.add_sized(
                        [80.0, 22.0],
                        egui::TextEdit::singleline(&mut self.transaction_draft.name)
                            .hint_text("名称"),
                    );
                }
                if matches!(
                    self.transaction_draft.kind,
                    TransactionKind::Buy | TransactionKind::Sell
                ) {
                    ui.label("数量");
                    ui.add(egui::DragValue::new(&mut self.transaction_draft.quantity).speed(100.0));
                    ui.label("价格");
                    ui.add(egui::DragValue::new(&mut self.transaction_draft.price).speed(0.1));
                } else {
                    ui.label("金额");
                    ui.add(
                        egui::DragValue::new(&mut self.transaction_draft.cash_amount).speed(100.0),
                    );
                }
                ui.label("费用");
                ui.add(
                    egui::DragValue::new(&mut self.transaction_draft.fees)
                        .speed(0.1)
                        .range(0.0..=1_000_000.0),
                );
                if ui.button("记录").clicked() {
                    self.submit_transaction();
                }
            });
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                Grid::new("transaction_history")
                    .num_columns(7)
                    .spacing(vec2(12.0, 5.0))
                    .striped(true)
                    .show(ui, |ui| {
                        for label in [
                            "日期",
                            "账户",
                            "类型",
                            "标的",
                            "数量",
                            "价格/依据",
                            "已实现",
                        ] {
                            table_header(ui, label);
                        }
                        ui.end_row();
                        for transaction in self.portfolio.transactions.iter().rev().take(30) {
                            let account = self
                                .portfolio
                                .accounts
                                .iter()
                                .find(|account| account.id == transaction.account_id)
                                .map(|account| {
                                    format!("{} {}", account.name, account.account_suffix)
                                })
                                .unwrap_or_else(|| "未知".into());
                            ui.label(format!(
                                "{}{}",
                                transaction.date,
                                if transaction.execution.date_is_estimated {
                                    "（归档）"
                                } else {
                                    ""
                                }
                            ));
                            ui.label(account);
                            ui.label(transaction.kind.label());
                            ui.label(if transaction.name.is_empty() {
                                &transaction.code
                            } else {
                                &transaction.name
                            });
                            ui.label(format!("{:.0}", transaction.quantity));
                            let price = match transaction.execution.price_basis.as_str() {
                                "fill" => format!("{:.3}", transaction.price),
                                "estimated" | "broker_cost" => {
                                    format!("约 {:.3}（待核）", transaction.price)
                                }
                                _ => "待核".into(),
                            };
                            ui.label(price).on_hover_text(&transaction.note);
                            ui.label(
                                RichText::new(format_money(
                                    transaction.realized_pnl.unwrap_or(f64::NAN),
                                ))
                                .color(pnl_color_for(
                                    transaction.realized_pnl.unwrap_or(0.0),
                                    palette,
                                )),
                            );
                            ui.end_row();
                        }
                    });
            });
        });
    }

    fn render_alerts_panel(&mut self, ui: &mut egui::Ui) {
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "价格与风险提醒", &mut self.active_panel);
            ui.horizontal_wrapped(|ui| {
                ui.label("代码");
                ui.add_sized(
                    [66.0, 22.0],
                    egui::TextEdit::singleline(&mut self.alert_draft.code),
                );
                ui.add_sized(
                    [80.0, 22.0],
                    egui::TextEdit::singleline(&mut self.alert_draft.name).hint_text("名称"),
                );
                egui::ComboBox::from_id_salt("alert_kind")
                    .selected_text(self.alert_draft.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in [
                            AlertKind::PriceAbove,
                            AlertKind::PriceBelow,
                            AlertKind::ChangeAbove,
                            AlertKind::ChangeBelow,
                            AlertKind::TodayPnlBelow,
                            AlertKind::DrawdownFromHigh,
                        ] {
                            ui.selectable_value(&mut self.alert_draft.kind, kind, kind.label());
                        }
                    });
                ui.label("阈值");
                ui.add(egui::DragValue::new(&mut self.alert_draft.threshold).speed(0.1));
                if ui.button("添加提醒").clicked() {
                    let code = digits6(&self.alert_draft.code);
                    if code.len() == 6 {
                        self.portfolio.alerts.push(AlertRule {
                            id: self.portfolio.next_id("alert"),
                            code: code.clone(),
                            name: self.alert_draft.name.trim().to_owned(),
                            market: Market::infer(&code),
                            kind: self.alert_draft.kind,
                            threshold: self.alert_draft.threshold,
                            enabled: true,
                            was_matching: false,
                            last_triggered_at: None,
                            high_watermark: self
                                .quotes
                                .quotes
                                .get(&code)
                                .map(|quote| quote.price)
                                .unwrap_or_default(),
                        });
                        let _ = config::save_portfolio(&mut self.portfolio);
                        self.alert_draft = AlertDraft::default();
                        self.start_fetch(true);
                    } else {
                        self.status = "提醒代码必须是 6 位".to_owned();
                    }
                }
                ui.separator();
                ui.label(format!("交易日历：{}", calendar::source_label()));
                if ui.button("从上交所更新").clicked() {
                    match calendar::refresh_from_sse(Local::now().year()) {
                        Ok(count) => self.status = format!("交易日历已更新，共 {count} 个休市日"),
                        Err(err) => self.status = format!("交易日历更新失败：{err:#}"),
                    }
                }
            });
            ui.add_space(6.0);
            let mut remove = None;
            ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                Grid::new("alert_rules")
                    .num_columns(7)
                    .spacing(vec2(12.0, 5.0))
                    .striped(true)
                    .show(ui, |ui| {
                        for label in ["启用", "标的", "条件", "阈值", "现价", "上次触发", ""]
                        {
                            table_header(ui, label);
                        }
                        ui.end_row();
                        for (index, alert) in self.portfolio.alerts.iter_mut().enumerate() {
                            ui.checkbox(&mut alert.enabled, "");
                            ui.label(if alert.name.is_empty() {
                                &alert.code
                            } else {
                                &alert.name
                            });
                            ui.label(alert.kind.label());
                            ui.label(format!("{:.2}", alert.threshold));
                            ui.label(
                                self.quotes
                                    .quotes
                                    .get(&alert.code)
                                    .map(|quote| format!("{:.2}", quote.price))
                                    .unwrap_or_else(|| "--".to_owned()),
                            );
                            ui.label(
                                alert
                                    .last_triggered_at
                                    .map(|time| time.format("%m-%d %H:%M").to_string())
                                    .unwrap_or_else(|| "--".to_owned()),
                            );
                            if ui.button("删除").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
            });
            if let Some(index) = remove {
                self.portfolio.alerts.remove(index);
                let _ = config::save_portfolio(&mut self.portfolio);
            }
        });
    }

    fn render_risk_panel(&mut self, ui: &mut egui::Ui) {
        let totals = self.totals();
        let palette = self.palette();
        let selected_owner = self.selected_owner.clone();
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "资产与风险", &mut self.active_panel);
            let cash_ratio = if totals.total_assets > 0.0 {
                self.cash_for_owner(&selected_owner) / totals.total_assets * 100.0
            } else {
                0.0
            };
            let has_snapshot_history = selected_owner == DEFAULT_OWNER;
            let monthly_return = has_snapshot_history
                .then(|| monthly_portfolio_return(&self.portfolio))
                .flatten();
            let benchmark_return = has_snapshot_history
                .then(|| monthly_benchmark_return(&self.portfolio.snapshots))
                .flatten();
            let max_drawdown = has_snapshot_history.then(|| self.portfolio.max_drawdown());
            ui.horizontal_wrapped(|ui| {
                text_metric_card(
                    ui,
                    "现金比例",
                    &format!("{cash_ratio:.1}%"),
                    palette.text_primary,
                    self.settings.font_scale,
                    palette,
                );
                text_metric_card(
                    ui,
                    "最大回撤",
                    &max_drawdown
                        .map(|value| format!("-{value:.1}%"))
                        .unwrap_or_else(|| "--".to_owned()),
                    max_drawdown
                        .map(|value| pnl_color_for(-value, palette))
                        .unwrap_or(palette.text_muted),
                    self.settings.font_scale,
                    palette,
                );
                text_metric_card(
                    ui,
                    "本月收益",
                    &monthly_return
                        .map(|value| format!("{value:+.2}%"))
                        .unwrap_or_else(|| "--".to_owned()),
                    monthly_return
                        .map(|value| pnl_color_for(value, palette))
                        .unwrap_or(palette.text_muted),
                    self.settings.font_scale,
                    palette,
                );
                text_metric_card(
                    ui,
                    "沪深300本月",
                    &benchmark_return
                        .map(|value| format!("{value:+.2}%"))
                        .unwrap_or_else(|| "--".to_owned()),
                    benchmark_return
                        .map(|value| pnl_color_for(value, palette))
                        .unwrap_or(palette.text_muted),
                    self.settings.font_scale,
                    palette,
                );
            });
            ui.horizontal(|ui| {
                ScrollArea::vertical().max_height(115.0).show(ui, |ui| {
                    Grid::new("risk_concentration")
                        .num_columns(4)
                        .spacing(vec2(12.0, 5.0))
                        .striped(true)
                        .show(ui, |ui| {
                            for label in ["标的", "市值", "资产占比", "提示"] {
                                table_header(ui, label);
                            }
                            ui.end_row();
                            for account in
                                self.portfolio.accounts.iter().filter(|account| {
                                    account_matches_owner(account, &selected_owner)
                                })
                            {
                                for holding in &account.holdings {
                                    let market_value = self
                                        .quotes
                                        .quotes
                                        .get(&holding.code)
                                        .map(|quote| quote.market_value(holding))
                                        .unwrap_or_default();
                                    let ratio = if totals.total_assets > 0.0 {
                                        market_value / totals.total_assets * 100.0
                                    } else {
                                        0.0
                                    };
                                    ui.label(&holding.name);
                                    ui.label(format_money(market_value));
                                    ui.label(format!("{ratio:.1}%"));
                                    ui.label(
                                        RichText::new(if ratio >= 50.0 {
                                            "高度集中"
                                        } else if ratio >= 30.0 {
                                            "偏集中"
                                        } else {
                                            "正常"
                                        })
                                        .color(
                                            if ratio >= 30.0 {
                                                palette.warning
                                            } else {
                                                palette.text_muted
                                            },
                                        ),
                                    );
                                    ui.end_row();
                                }
                            }
                        });
                });
                if has_snapshot_history {
                    draw_snapshot_chart(ui, &self.portfolio.snapshots);
                } else {
                    ui.label(RichText::new("托管资金暂不记录历史曲线").color(palette.text_muted));
                }
            });
            let mut account_values = BTreeMap::<String, f64>::new();
            let mut market_values = BTreeMap::<&'static str, f64>::new();
            for account in self
                .portfolio
                .accounts
                .iter()
                .filter(|account| account_matches_owner(account, &selected_owner))
            {
                let value = account
                    .holdings
                    .iter()
                    .filter_map(|holding| {
                        self.quotes
                            .quotes
                            .get(&holding.code)
                            .map(|quote| quote.market_value(holding))
                    })
                    .sum::<f64>();
                account_values.insert(account.name.clone(), value);
                for holding in &account.holdings {
                    let market_value = self
                        .quotes
                        .quotes
                        .get(&holding.code)
                        .map(|quote| quote.market_value(holding))
                        .unwrap_or_default();
                    *market_values.entry(holding.market.label()).or_default() += market_value;
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("账户").color(palette.text_muted));
                for (name, value) in account_values {
                    ui.label(format!(
                        "{name} {:.1}%",
                        value / totals.total_assets.max(1.0) * 100.0
                    ));
                }
                ui.separator();
                ui.label(RichText::new("市场").color(palette.text_muted));
                for (market, value) in market_values {
                    ui.label(format!(
                        "{market} {:.1}%",
                        value / totals.total_assets.max(1.0) * 100.0
                    ));
                }
            });
        });
    }

    fn render_watchlist_panel(&mut self, ui: &mut egui::Ui) {
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "自选观察", &mut self.active_panel);
            ui.horizontal(|ui| {
                ui.label("代码");
                ui.add_sized(
                    [66.0, 22.0],
                    egui::TextEdit::singleline(&mut self.watch_code),
                );
                ui.add_sized(
                    [90.0, 22.0],
                    egui::TextEdit::singleline(&mut self.watch_name).hint_text("名称"),
                );
                if ui.button("加入自选").clicked() {
                    let code = digits6(&self.watch_code);
                    if code.len() == 6 {
                        self.portfolio.watchlist.push(WatchItem {
                            code: code.clone(),
                            name: self.watch_name.trim().to_owned(),
                            market: Market::infer(&code),
                        });
                        self.portfolio.normalize();
                        let _ = config::save_portfolio(&mut self.portfolio);
                        self.watch_code.clear();
                        self.watch_name.clear();
                        self.start_fetch(true);
                    }
                }
            });
            let mut remove = None;
            ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                Grid::new("watchlist_grid")
                    .num_columns(6)
                    .spacing(vec2(14.0, 5.0))
                    .striped(true)
                    .show(ui, |ui| {
                        for label in ["名称", "现价", "涨跌幅", "成交额", "", ""] {
                            table_header(ui, label);
                        }
                        ui.end_row();
                        for (index, item) in self.portfolio.watchlist.iter().enumerate() {
                            let quote = self.quotes.quotes.get(&item.code);
                            ui.label(if item.name.is_empty() {
                                &item.code
                            } else {
                                &item.name
                            });
                            ui.label(
                                quote
                                    .map(|quote| format!("{:.3}", quote.price))
                                    .unwrap_or_else(|| "--".to_owned()),
                            );
                            ui.label(
                                quote
                                    .map(|quote| format!("{:+.2}%", quote.change_percent))
                                    .unwrap_or_else(|| "--".to_owned()),
                            );
                            ui.label(
                                quote
                                    .and_then(|quote| quote.amount)
                                    .map(format_large_number)
                                    .unwrap_or_else(|| "--".to_owned()),
                            );
                            if ui.button("详情").clicked() {
                                self.selected_detail_code = Some(item.code.clone());
                                self.active_panel = Some(ToolPanel::Detail);
                            }
                            if ui.button("删除").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
            });
            if let Some(index) = remove {
                self.portfolio.watchlist.remove(index);
                let _ = config::save_portfolio(&mut self.portfolio);
            }
        });
    }

    fn render_backups_panel(&mut self, ui: &mut egui::Ui) {
        let backups = config::list_portfolio_backups();
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "自动备份与恢复", &mut self.active_panel);
            ui.horizontal(|ui| {
                ui.label(format!("保存前自动备份，当前保留 {} 份", backups.len()));
                if ui.button("恢复上一版").clicked() {
                    match config::restore_latest_portfolio_backup() {
                        Ok(portfolio) => {
                            self.portfolio = portfolio;
                            self.status = "已恢复上一版持仓，可继续检查或再次恢复".to_owned();
                            self.start_fetch(true);
                        }
                        Err(err) => self.status = format!("恢复失败：{err:#}"),
                    }
                }
            });
            ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                for path in backups.iter().take(20) {
                    ui.horizontal(|ui| {
                        ui.label(
                            path.file_stem()
                                .and_then(|name| name.to_str())
                                .unwrap_or("备份"),
                        );
                        if ui.small_button("恢复此版").clicked() {
                            match config::restore_portfolio_backup(path) {
                                Ok(portfolio) => {
                                    self.portfolio = portfolio;
                                    self.status = "指定备份已恢复".to_owned();
                                    self.start_fetch(true);
                                }
                                Err(err) => self.status = format!("恢复失败：{err:#}"),
                            }
                        }
                    });
                }
            });
        });
    }

    fn render_detail_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let code = self.selected_detail_code.clone().unwrap_or_default();
        let quote = self.quotes.quotes.get(&code).cloned();
        tool_frame(ui, |ui| {
            tool_panel_header(ui, "股票详情", &mut self.active_panel);
            let Some(quote) = quote else {
                ui.label("尚无该股票行情");
                return;
            };
            ui.horizontal_wrapped(|ui| {
                ui.heading(format!("{} {}", quote.name, quote.code));
                ui.label(
                    RichText::new(format!("{:.3}  {:+.2}%", quote.price, quote.change_percent))
                        .color(pnl_color_for(quote.change_percent, palette))
                        .strong(),
                );
                detail_value(ui, "昨收", Some(quote.previous_close));
                detail_value(ui, "今开", quote.open);
                detail_value(ui, "最高", quote.high);
                detail_value(ui, "最低", quote.low);
                detail_text(ui, "成交量", quote.volume.map(format_large_number));
                detail_text(ui, "成交额", quote.amount.map(format_large_number));
                detail_text(
                    ui,
                    "换手率",
                    quote.turnover_rate.map(|value| format!("{value:.2}%")),
                );
                detail_value(ui, "市盈率", quote.pe_ratio);
                detail_text(ui, "总市值", quote.market_cap.map(format_large_number));
                ui.label(
                    RichText::new(format!(
                        "更新 {}",
                        quote.updated_at.format("%Y-%m-%d %H:%M:%S")
                    ))
                    .small()
                    .color(palette.text_muted),
                );
            });
        });
    }

    fn render_ocr_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(palette.radius)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("截图 OCR")
                                .strong()
                                .color(palette.text_primary),
                        );
                        ui.label(
                            RichText::new("复制持仓截图后粘贴，或把图片拖进下面的框。")
                                .color(palette.text_secondary),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("关闭").clicked() {
                            self.active_panel = None;
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

                if let Some(result) = self.pending_import.clone() {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(palette.surface_alt)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(palette.radius)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("识别差异预览")
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                account_picker(
                                    ui,
                                    "import_account",
                                    &self.portfolio,
                                    &mut self.import_account_index,
                                );
                                ui.checkbox(
                                    &mut self.import_replace_missing,
                                    "截图未出现的持仓视为清仓",
                                );
                                if ui.button("确认应用").clicked() {
                                    self.apply_pending_import();
                                }
                                if ui.button("放弃").clicked() {
                                    self.pending_import = None;
                                }
                            });
                            if let Some(cash) = result.cash {
                                ui.label(format!("识别可用资金：{}", format_money(cash)));
                            }
                            if let Some(today_pnl) = result.today_pnl {
                                ui.label(
                                    RichText::new(format!(
                                        "截图今日盈亏：{}",
                                        format_money(today_pnl)
                                    ))
                                    .color(pnl_color_for(today_pnl, palette)),
                                );
                            }
                            ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                Grid::new("import_preview")
                                    .num_columns(5)
                                    .spacing(vec2(12.0, 4.0))
                                    .striped(true)
                                    .show(ui, |ui| {
                                        for label in
                                            ["标的", "原数量", "新数量", "原成本", "新成本"]
                                        {
                                            table_header(ui, label);
                                        }
                                        ui.end_row();
                                        let account =
                                            self.portfolio.accounts.get(self.import_account_index);
                                        for holding in &result.holdings {
                                            let previous = account.and_then(|account| {
                                                account
                                                    .holdings
                                                    .iter()
                                                    .find(|old| old.code == holding.code)
                                            });
                                            ui.label(&holding.name);
                                            ui.label(
                                                previous
                                                    .map(|old| format!("{:.0}", old.quantity))
                                                    .unwrap_or_else(|| "0".to_owned()),
                                            );
                                            ui.label(format!("{:.0}", holding.quantity));
                                            ui.label(
                                                previous
                                                    .map(|old| format!("{:.3}", old.cost_price))
                                                    .unwrap_or_else(|| "--".to_owned()),
                                            );
                                            ui.label(format!("{:.3}", holding.cost_price));
                                            ui.end_row();
                                        }
                                    });
                            });
                        });
                }

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
        let palette = self.palette();
        let row_height = 28.0;
        let columns = if self.editing { 12 } else { 6 };
        let mut open_detail = None;
        let selected_owner = self.selected_owner.clone();
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
                        let mut visible_holdings = 0usize;
                        for (account_idx, account) in self.portfolio.accounts.iter_mut().enumerate()
                        {
                            if !account_matches_owner(account, &selected_owner) {
                                continue;
                            }
                            if self.editing {
                                ui.add_sized(
                                    [84.0, row_height],
                                    egui::TextEdit::singleline(&mut account.name),
                                );
                                ui.add_sized(
                                    [58.0, row_height],
                                    egui::TextEdit::singleline(&mut account.owner)
                                        .hint_text("归属"),
                                );
                                for _ in 2..columns {
                                    ui.label("");
                                }
                                ui.end_row();
                            }

                            for (holding_idx, holding) in account.holdings.iter_mut().enumerate() {
                                visible_holdings += 1;
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
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(display_name)
                                                    .color(palette.text_primary),
                                            )
                                            .sense(Sense::click()),
                                        )
                                        .on_hover_text("查看行情详情")
                                        .clicked()
                                    {
                                        open_detail = Some(holding.code.clone());
                                    }
                                }

                                if let Some(q) = quote {
                                    ui.label(format!("{:.3}", q.price));
                                    ui.label(
                                        RichText::new(format!("{:+.2}%", q.change_percent))
                                            .color(pnl_color_for(q.change_percent, palette)),
                                    );
                                    ui.label(
                                        RichText::new(format_money(q.position_pnl(holding)))
                                            .color(pnl_color_for(q.position_pnl(holding), palette)),
                                    );
                                    ui.label(
                                        RichText::new(format_money(q.today_pnl(holding)))
                                            .color(pnl_color_for(q.today_pnl(holding), palette)),
                                    );
                                    ui.label(
                                        RichText::new(format_money(q.market_value(holding)))
                                            .color(palette.text_secondary),
                                    );
                                } else {
                                    for _ in 0..5 {
                                        ui.label(RichText::new("--").color(palette.text_muted));
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
                                    egui::ComboBox::from_id_salt(format!(
                                        "market_{account_idx}_{holding_idx}"
                                    ))
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
                                        remove_idx = Some((account_idx, holding_idx));
                                    }
                                }
                                ui.end_row();
                            }
                        }
                        if visible_holdings == 0
                            && !self.editing
                            && !self.portfolio.watchlist.is_empty()
                        {
                            for item in &self.portfolio.watchlist {
                                let quote = self.quotes.quotes.get(&item.code);
                                if ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(&item.name).color(palette.text_primary),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .on_hover_text("查看行情详情")
                                    .clicked()
                                {
                                    open_detail = Some(item.code.clone());
                                }
                                if let Some(q) = quote {
                                    ui.label(format!("{:.3}", q.price));
                                    ui.label(
                                        RichText::new(format!("{:+.2}%", q.change_percent))
                                            .color(pnl_color_for(q.change_percent, palette)),
                                    );
                                } else {
                                    ui.label("--");
                                    ui.label("--");
                                }
                                for _ in 0..3 {
                                    ui.label(RichText::new("--").color(palette.text_muted));
                                }
                                ui.end_row();
                            }
                        } else if visible_holdings == 0 {
                            ui.label(
                                RichText::new(format!("{} 暂无持仓", selected_owner))
                                    .color(palette.text_muted),
                            );
                            for _ in 1..columns {
                                ui.label("");
                            }
                            ui.end_row();
                        }

                        if let Some((account_idx, holding_idx)) = remove_idx {
                            if let Some(account) = self.portfolio.accounts.get_mut(account_idx) {
                                account.holdings.remove(holding_idx);
                            }
                        }
                    });
            });
        if let Some(code) = open_detail {
            self.selected_detail_code = Some(code);
            self.active_panel = Some(ToolPanel::Detail);
        }
    }
}

fn panel_toggle(ui: &mut egui::Ui, active: &mut Option<ToolPanel>, panel: ToolPanel, label: &str) {
    if ui.selectable_label(*active == Some(panel), label).clicked() {
        *active = if *active == Some(panel) {
            None
        } else {
            Some(panel)
        };
    }
}

fn account_matches_owner(account: &Account, owner: &str) -> bool {
    let owner = owner.trim();
    let owner = if owner.is_empty() {
        DEFAULT_OWNER
    } else {
        owner
    };
    if is_combined_owner(owner) {
        account.owner_label() == DEFAULT_OWNER || account.owner_label() == "B"
    } else {
        account.owner_label() == owner
    }
}

fn is_combined_owner(owner: &str) -> bool {
    owner.trim() == COMBINED_OWNER
}

fn tool_frame(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let visuals = ui.visuals();
    let fill = visuals.window_fill;
    let stroke = visuals.window_stroke;
    let radius = visuals.window_corner_radius;
    Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(radius)
        .inner_margin(10.0)
        .show(ui, content);
}

fn tool_panel_header(ui: &mut egui::Ui, title: &str, active: &mut Option<ToolPanel>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("关闭").clicked() {
                *active = None;
            }
        });
    });
    ui.add_space(5.0);
}

fn account_picker(ui: &mut egui::Ui, id: &str, portfolio: &Portfolio, selected: &mut usize) {
    if portfolio.accounts.is_empty() {
        ui.label("无账户");
        return;
    }
    *selected = (*selected).min(portfolio.accounts.len() - 1);
    egui::ComboBox::from_id_salt(id)
        .selected_text(&portfolio.accounts[*selected].name)
        .show_ui(ui, |ui| {
            for (index, account) in portfolio.accounts.iter().enumerate() {
                ui.selectable_value(selected, index, &account.name);
            }
        });
}

fn draw_snapshot_chart(ui: &mut egui::Ui, snapshots: &[DailySnapshot]) {
    let visuals = ui.visuals();
    let chart_fill = visuals.faint_bg_color;
    let chart_line = visuals.hyperlink_color;
    let text_color = visuals.weak_text_color();
    let width = ui.available_width().clamp(170.0, 300.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 110.0), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, chart_fill);
    let recent = snapshots.iter().rev().take(60).collect::<Vec<_>>();
    if recent.len() < 2 {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "积累两个交易日后显示净值曲线",
            FontId::proportional(12.0),
            text_color,
        );
        return;
    }
    let min = recent
        .iter()
        .map(|snapshot| snapshot.total_assets)
        .fold(f64::INFINITY, f64::min);
    let max = recent
        .iter()
        .map(|snapshot| snapshot.total_assets)
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(1.0);
    let points = recent
        .iter()
        .rev()
        .enumerate()
        .map(|(index, snapshot)| {
            let x =
                rect.left() + rect.width() * index as f32 / (recent.len().saturating_sub(1)) as f32;
            let y = rect.bottom() - rect.height() * ((snapshot.total_assets - min) / span) as f32;
            egui::pos2(x, y)
        })
        .collect::<Vec<_>>();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.8, chart_line)));
    ui.painter().text(
        rect.left_top() + vec2(6.0, 5.0),
        egui::Align2::LEFT_TOP,
        format!("净值  {:.2}万", max / 10_000.0),
        FontId::proportional(11.0),
        text_color,
    );
}

fn detail_value(ui: &mut egui::Ui, label: &str, value: Option<f64>) {
    detail_text(ui, label, value.map(|value| format!("{value:.3}")));
}

fn detail_text(ui: &mut egui::Ui, label: &str, value: Option<String>) {
    let color = ui.visuals().weak_text_color();
    ui.label(
        RichText::new(format!(
            "{label} {}",
            value.unwrap_or_else(|| "--".to_owned())
        ))
        .color(color),
    );
}

fn digits6(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(6)
        .collect()
}

fn format_large_number(value: f64) -> String {
    if value.abs() >= 100_000_000.0 {
        format!("{:.2}亿", value / 100_000_000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.2}万", value / 10_000.0)
    } else {
        format!("{value:.2}")
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
            .push("msyh".to_owned());
    }
    let monospace_path = [
        "C:\\Windows\\Fonts\\CascadiaMono.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).exists());
    if let Some(path) = monospace_path {
        if let Ok(bytes) = fs::read(path) {
            fonts.font_data.insert(
                "app-monospace".to_owned(),
                FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "app-monospace".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context, theme: ThemePreset, scale: f32) {
    let scale = scale.clamp(0.8, 1.35);
    let palette = theme.palette();
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0 * scale)),
        (TextStyle::Body, FontId::proportional(13.5 * scale)),
        (TextStyle::Monospace, FontId::monospace(13.0 * scale)),
        (TextStyle::Button, FontId::proportional(13.0 * scale)),
        (TextStyle::Small, FontId::proportional(11.0 * scale)),
    ]
    .into();
    let radius = egui::CornerRadius::same(palette.radius.round() as u8);
    style.visuals.dark_mode = true;
    style.visuals.override_text_color = None;
    style.visuals.panel_fill = palette.background;
    style.visuals.window_fill = palette.surface;
    style.visuals.extreme_bg_color = palette.background;
    style.visuals.faint_bg_color = palette.surface_alt;
    style.visuals.code_bg_color = palette.surface_alt;
    style.visuals.warn_fg_color = palette.warning;
    style.visuals.error_fg_color = palette.gain;
    style.visuals.hyperlink_color = palette.accent;
    style.visuals.window_stroke = Stroke::new(1.0, palette.border);
    style.visuals.window_corner_radius = radius;
    style.visuals.menu_corner_radius = radius;
    style.visuals.selection.bg_fill = palette.accent;
    style.visuals.selection.stroke = Stroke::new(1.0, palette.text_primary);
    style.visuals.widgets.noninteractive.bg_fill = palette.surface;
    style.visuals.widgets.noninteractive.weak_bg_fill = palette.surface;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text_primary);
    style.visuals.widgets.noninteractive.corner_radius = radius;
    style.visuals.widgets.inactive.bg_fill = palette.surface_alt;
    style.visuals.widgets.inactive.weak_bg_fill = palette.surface_alt;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text_secondary);
    style.visuals.widgets.inactive.corner_radius = radius;
    style.visuals.widgets.hovered.bg_fill = palette.border;
    style.visuals.widgets.hovered.weak_bg_fill = palette.border;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text_primary);
    style.visuals.widgets.hovered.corner_radius = radius;
    style.visuals.widgets.active.bg_fill = palette.accent;
    style.visuals.widgets.active.weak_bg_fill = palette.accent;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text_primary);
    style.visuals.widgets.active.corner_radius = radius;
    style.visuals.widgets.open.bg_fill = palette.surface_alt;
    style.visuals.widgets.open.weak_bg_fill = palette.surface_alt;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, palette.accent);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, palette.text_primary);
    style.visuals.widgets.open.corner_radius = radius;
    style.spacing.button_padding = vec2(10.0, 5.0);
    ctx.set_style(style);
}

fn pnl_color_for(value: f64, palette: ThemePalette) -> Color32 {
    if value.abs() < 0.005 {
        palette.flat
    } else if value > 0.0 {
        palette.gain
    } else {
        palette.loss
    }
}

fn draw_ocr_drop_zone(ui: &egui::Ui, rect: Rect) {
    let visuals = ui.visuals();
    ui.painter().rect_filled(rect, 8.0, visuals.faint_bg_color);
    ui.painter().rect_stroke(
        rect.shrink(1.0),
        8.0,
        visuals.window_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "粘贴 / 拖放 / 点击选择 同花顺或东方财富持仓截图",
        FontId::proportional(14.0),
        visuals.weak_text_color(),
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

fn metric_card(
    ui: &mut egui::Ui,
    label: &str,
    value: f64,
    color: Color32,
    scale: f32,
    percent: Option<f64>,
    palette: ThemePalette,
) {
    let frame = Frame::new()
        .fill(palette.surface_alt)
        .stroke(Stroke::new(1.0, palette.card_border))
        .corner_radius(palette.radius)
        .inner_margin(8.0);

    frame.show(ui, |ui| {
        ui.set_min_size(vec2(108.0, 34.0));
        ui.label(
            RichText::new(label)
                .size(10.0 * scale)
                .color(palette.text_muted),
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format_money(value))
                    .monospace()
                    .size(15.5 * scale)
                    .strong()
                    .color(color),
            );
            if let Some(percent) = percent {
                ui.label(
                    RichText::new(format_percent(percent))
                        .monospace()
                        .size(10.5 * scale)
                        .color(color),
                );
            }
        });
    });
    ui.add_space(8.0);
}

fn today_pnl_percent_label(
    ui: &mut egui::Ui,
    totals: PortfolioTotals,
    scale: f32,
    palette: ThemePalette,
) {
    if let Some(percent) = totals.today_pnl_percent() {
        ui.label(
            RichText::new(format_percent(percent))
                .monospace()
                .size(10.5 * scale)
                .color(pnl_color_for(totals.today_pnl, palette)),
        );
    }
}

fn position_badge(ui: &mut egui::Ui, percent: Option<f64>, scale: f32, palette: ThemePalette) {
    Frame::new()
        .fill(palette.badge)
        .stroke(Stroke::new(1.0, palette.card_border))
        .corner_radius(palette.radius)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "仓位 {}",
                    percent
                        .map(|value| format!("{value:.1}%"))
                        .unwrap_or_else(|| "--".to_owned())
                ))
                .monospace()
                .size(10.5 * scale)
                .color(palette.text_secondary),
            );
        });
}

fn text_metric_card(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    color: Color32,
    scale: f32,
    palette: ThemePalette,
) {
    Frame::new()
        .fill(palette.surface_alt)
        .stroke(Stroke::new(1.0, palette.card_border))
        .corner_radius(palette.radius)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_min_size(vec2(108.0, 34.0));
            ui.label(
                RichText::new(label)
                    .size(10.0 * scale)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(value)
                    .monospace()
                    .size(15.5 * scale)
                    .strong()
                    .color(color),
            );
        });
    ui.add_space(8.0);
}

fn monthly_portfolio_return(portfolio: &Portfolio) -> Option<f64> {
    let today = Local::now().date_naive();
    let month = portfolio
        .snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.date.year() == today.year() && snapshot.date.month() == today.month()
        })
        .collect::<Vec<_>>();
    let first = month.first()?;
    let last = month.last()?;
    if month.len() < 2 || first.total_assets <= 0.0 {
        return None;
    }
    let net_flow = portfolio
        .transactions
        .iter()
        .filter(|transaction| {
            transaction.date > first.date
                && transaction.date <= last.date
                && portfolio
                    .accounts
                    .iter()
                    .any(|a| a.id == transaction.account_id && a.owner_label() == DEFAULT_OWNER)
                && matches!(
                    transaction.kind,
                    TransactionKind::Deposit | TransactionKind::Withdraw
                )
        })
        .map(|transaction| match transaction.kind {
            TransactionKind::Deposit => transaction.cash_amount.abs(),
            TransactionKind::Withdraw => -transaction.cash_amount.abs(),
            _ => 0.0,
        })
        .sum::<f64>();
    Some((last.total_assets - first.total_assets - net_flow) / first.total_assets * 100.0)
}

fn monthly_benchmark_return(snapshots: &[DailySnapshot]) -> Option<f64> {
    let today = Local::now().date_naive();
    let month = snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.date.year() == today.year() && snapshot.date.month() == today.month()
        })
        .collect::<Vec<_>>();
    let first = month.first()?.benchmark_level?;
    let last = month.last()?.benchmark_level?;
    (month.len() >= 2 && first.is_finite() && first > 0.0 && last.is_finite())
        .then_some((last / first - 1.0) * 100.0)
}

fn table_header(ui: &mut egui::Ui, text: &str) {
    let color = ui.visuals().weak_text_color();
    ui.label(RichText::new(text).small().strong().color(color));
}

fn format_money(value: f64) -> String {
    if !value.is_finite() {
        return "待核".into();
    }
    if value.abs() >= 10_000.0 {
        format!("{:.2}万", value / 10_000.0)
    } else {
        format!("{value:.2}")
    }
}

fn format_percent(value: f64) -> String {
    if !value.is_finite() {
        return "待核".into();
    }
    format!("{value:+.2}%")
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
    if !calendar::is_trading_day(now.date()) {
        return false;
    }

    let time = now.time();
    let morning_start = market_time(9, 15);
    let morning_end = market_time(11, 30);
    let afternoon_start = market_time(13, 0);
    let afternoon_end = market_time(15, 0);

    (time >= morning_start && time <= morning_end)
        || (time >= afternoon_start && time <= afternoon_end)
}

fn next_market_open_after(now: NaiveDateTime) -> NaiveDateTime {
    let morning_start = market_time(9, 15);
    let afternoon_start = market_time(13, 0);
    let morning_end = market_time(11, 30);
    let afternoon_end = market_time(15, 0);

    if calendar::is_trading_day(now.date()) {
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
    while !calendar::is_trading_day(date) {
        date += chrono::Duration::days(1);
    }
    date.and_time(morning_start)
}

fn market_time(hour: u32, minute: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(hour, minute, 0).expect("valid market time")
}

fn format_duration_for_status(duration: Duration) -> String {
    if duration.as_secs() < 60 {
        return format!("{}秒", duration.as_secs());
    }

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
    fn monthly_returns_use_same_owner_flows_and_endpoint_levels() {
        let mut p = Portfolio::default();
        let mut other = p.accounts[0].clone();
        other.id = "other".into();
        other.owner = "B".into();
        p.accounts.push(other);
        let start = Local::now().date_naive().with_day(1).unwrap();
        let end = start.with_day(3).unwrap();
        for (date, assets, level) in [(start, 100.0, 100.0), (end, 120.0, 110.0)] {
            p.snapshots.push(DailySnapshot {
                date,
                total_assets: assets,
                market_value: 0.0,
                cash: assets,
                position_pnl: 0.0,
                today_pnl: 0.0,
                benchmark_level: Some(level),
                benchmark_change_percent: Some(5.0),
            });
        }
        for (id, account, date) in [
            ("first", p.accounts[0].id.clone(), start),
            ("other", "other".into(), end),
        ] {
            p.transactions.push(
                serde_json::from_value(serde_json::json!({"id": id,
                "account_id": account, "date": date, "kind": "Deposit", "cash_amount": 1000.0}))
                .unwrap(),
            );
        }
        assert!((monthly_portfolio_return(&p).unwrap() - 20.0).abs() < 1e-6);
        assert!((monthly_benchmark_return(&p.snapshots).unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn market_session_starts_at_call_auction_time() {
        assert!(!is_market_session_time(dt("2026-04-29", 9, 14)));
        assert!(is_market_session_time(dt("2026-04-29", 9, 15)));
        assert!(is_market_session_time(dt("2026-04-29", 11, 30)));
    }

    #[test]
    fn today_pnl_percent_uses_previous_assets() {
        let totals = PortfolioTotals {
            total_assets: 512_781.31,
            today_pnl: 15_306.32,
            ..Default::default()
        };

        assert!((totals.today_pnl_percent().unwrap() - 3.0768).abs() < 0.001);
    }

    #[test]
    fn today_pnl_percent_skips_zero_previous_assets() {
        let totals = PortfolioTotals {
            total_assets: 100.0,
            today_pnl: 100.0,
            ..Default::default()
        };

        assert!(totals.today_pnl_percent().is_none());
    }

    #[test]
    fn position_percent_uses_market_value_over_total_assets() {
        let totals = PortfolioTotals {
            total_assets: 424_400.89,
            market_value: 230_100.0,
            ..Default::default()
        };

        assert!((totals.position_percent().unwrap() - 54.2176).abs() < 0.001);
    }

    #[test]
    fn position_percent_skips_non_positive_assets() {
        assert!(PortfolioTotals::default().position_percent().is_none());
    }

    #[test]
    fn combined_owner_contains_default_and_b_only() {
        let account = |owner: &str| Account {
            id: owner.to_owned(),
            name: owner.to_owned(),
            owner: owner.to_owned(),
            account_suffix: String::new(),
            cash: 0.0,
            today_realized_pnl: 0.0,
            today_realized_pnl_date: None,
            holdings: Vec::new(),
        };

        assert!(account_matches_owner(
            &account(DEFAULT_OWNER),
            COMBINED_OWNER
        ));
        assert!(account_matches_owner(&account("B"), COMBINED_OWNER));
        assert!(!account_matches_owner(&account("A"), COMBINED_OWNER));
        assert!(!account_matches_owner(&account("C"), COMBINED_OWNER));
    }

    #[test]
    fn theme_ids_round_trip_and_unknown_falls_back_to_copper() {
        for theme in ThemePreset::ALL {
            assert_eq!(ThemePreset::from_id(theme.id()), theme);
        }
        assert_eq!(ThemePreset::from_id("unknown"), ThemePreset::Copper);
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
            dt("2026-04-30", 9, 15)
        );
        assert_eq!(
            next_market_open_after(dt("2026-05-02", 10, 0)),
            dt("2026-05-06", 9, 15)
        );
    }

    #[test]
    fn market_session_skips_exchange_holidays() {
        assert!(!is_market_session_time(dt("2026-06-19", 10, 0)));
        assert_eq!(
            next_market_open_after(dt("2026-06-19", 10, 0)),
            dt("2026-06-22", 9, 15)
        );
    }
}
