use anyhow::{bail, Context};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const DEFAULT_OWNER: &str = "我的";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Holding {
    pub code: String,
    pub name: String,
    pub quantity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_quantity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intraday_cost_price: Option<f64>,
    pub cost_price: f64,
    #[serde(default = "default_market")]
    pub market: Market,
}

impl Holding {
    pub fn overnight_quantity_for(&self, quote_date: NaiveDate) -> f64 {
        if self.available_date == Some(quote_date) {
            return self
                .available_quantity
                .unwrap_or(self.quantity)
                .clamp(0.0, self.quantity);
        }

        self.quantity
    }

    pub fn intraday_quantity_for(&self, quote_date: NaiveDate) -> f64 {
        (self.quantity - self.overnight_quantity_for(quote_date)).max(0.0)
    }

    pub fn intraday_cost_price_for(&self, quote_date: NaiveDate) -> f64 {
        if self.available_date == Some(quote_date) {
            return self
                .intraday_cost_price
                .filter(|price| price.is_finite() && *price > 0.0)
                .unwrap_or(self.cost_price);
        }

        self.cost_price
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Market {
    Shanghai,
    Shenzhen,
    Beijing,
}

impl Market {
    pub fn eastmoney_prefix(self) -> &'static str {
        match self {
            Self::Shanghai => "1",
            Self::Shenzhen => "0",
            Self::Beijing => "0",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Shanghai => "沪",
            Self::Shenzhen => "深",
            Self::Beijing => "北",
        }
    }

    pub fn infer(code: &str) -> Self {
        let clean = code.trim();
        if clean.starts_with('6') || clean.starts_with('9') {
            Self::Shanghai
        } else if clean.starts_with('8') || clean.starts_with('4') {
            Self::Beijing
        } else {
            Self::Shenzhen
        }
    }
}

fn default_market() -> Market {
    Market::Shenzhen
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    #[serde(skip)]
    pub loaded_from: Option<String>,
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub alerts: Vec<AlertRule>,
    #[serde(default)]
    pub watchlist: Vec<WatchItem>,
    #[serde(default)]
    pub snapshots: Vec<DailySnapshot>,
    #[serde(default)]
    pub last_saved_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(
        default = "default_account_owner",
        skip_serializing_if = "is_default_owner"
    )]
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_suffix: String,
    pub cash: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub today_realized_pnl: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today_realized_pnl_date: Option<NaiveDate>,
    pub holdings: Vec<Holding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub id: String,
    pub account_id: String,
    pub date: NaiveDate,
    pub kind: TransactionKind,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_market")]
    pub market: Market,
    #[serde(default)]
    pub quantity: f64,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub fees: f64,
    #[serde(default)]
    pub cash_amount: f64,
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    #[serde(default)]
    pub execution: ExecutionEvidence,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExecutionEvidence {
    pub confidence: String,
    pub price_basis: String,
    pub fees_known: bool,
    pub date_is_estimated: bool,
    pub executed_at: Option<DateTime<Local>>,
    pub observed_at: Option<DateTime<Local>>,
}

impl Default for ExecutionEvidence {
    fn default() -> Self {
        Self {
            confidence: "Unknown".into(),
            price_basis: "unknown".into(),
            fees_known: false,
            date_is_estimated: true,
            executed_at: None,
            observed_at: None,
        }
    }
}

impl Transaction {
    pub fn daily_pnl(&self, previous_close: Option<f64>) -> Option<f64> {
        if self.execution.confidence != "Confirmed"
            || !self.execution.fees_known
            || self.execution.date_is_estimated
        {
            return None;
        }
        match self.kind {
            TransactionKind::Sell => previous_close
                .filter(|p| p.is_finite() && *p > 0.0)
                .map(|p| (self.price - p) * self.quantity - self.fees),
            TransactionKind::Dividend => Some(self.cash_amount - self.fees),
            TransactionKind::Fee => Some(-self.cash_amount.abs() - self.fees),
            _ => Some(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionKind {
    Buy,
    Sell,
    Dividend,
    Fee,
    Deposit,
    Withdraw,
    Adjustment,
}

impl TransactionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Buy => "买入",
            Self::Sell => "卖出",
            Self::Dividend => "分红",
            Self::Fee => "费用",
            Self::Deposit => "入金",
            Self::Withdraw => "出金",
            Self::Adjustment => "持仓校准",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertRule {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default = "default_market")]
    pub market: Market,
    pub kind: AlertKind,
    pub threshold: f64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub was_matching: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_triggered_at: Option<DateTime<Local>>,
    #[serde(default)]
    pub high_watermark: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlertKind {
    PriceAbove,
    PriceBelow,
    ChangeAbove,
    ChangeBelow,
    TodayPnlBelow,
    DrawdownFromHigh,
}

impl AlertKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PriceAbove => "价格高于",
            Self::PriceBelow => "价格低于",
            Self::ChangeAbove => "涨幅高于",
            Self::ChangeBelow => "跌幅低于",
            Self::TodayPnlBelow => "今日亏损超过",
            Self::DrawdownFromHigh => "高点回撤超过",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchItem {
    pub code: String,
    pub name: String,
    #[serde(default = "default_market")]
    pub market: Market,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailySnapshot {
    pub date: NaiveDate,
    pub total_assets: f64,
    pub market_value: f64,
    pub cash: f64,
    pub position_pnl: f64,
    pub today_pnl: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_level: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_change_percent: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ReconcileSummary {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

impl Account {
    pub fn owner_label(&self) -> &str {
        if self.owner.trim().is_empty() {
            DEFAULT_OWNER
        } else {
            self.owner.trim()
        }
    }

    pub fn today_realized_pnl_for(&self, quote_date: NaiveDate) -> f64 {
        if self.today_realized_pnl_date == Some(quote_date) {
            self.today_realized_pnl
        } else {
            0.0
        }
    }
}

impl Default for Portfolio {
    fn default() -> Self {
        Self {
            loaded_from: None,
            accounts: vec![Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                owner: DEFAULT_OWNER.to_owned(),
                account_suffix: String::new(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings: Vec::new(),
            }],
            transactions: Vec::new(),
            alerts: Vec::new(),
            watchlist: Vec::new(),
            snapshots: Vec::new(),
            last_saved_at: None,
        }
    }
}

impl Portfolio {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.accounts.is_empty() {
            bail!("账户列表为空");
        }
        let mut ids = HashSet::new();
        let mut suffixes = HashSet::new();
        for account in &self.accounts {
            if account.id.trim().is_empty() || !ids.insert(account.id.as_str()) {
                bail!("账户 ID 为空或重复");
            }
            if !account.account_suffix.is_empty()
                && !suffixes.insert(account.account_suffix.as_str())
            {
                bail!("账户尾号重复");
            }
            if !account.cash.is_finite() || !account.today_realized_pnl.is_finite() {
                bail!("账户金额无效");
            }
            let mut codes = HashSet::new();
            for h in &account.holdings {
                if h.code.len() != 6
                    || !h.code.chars().all(|c| c.is_ascii_digit())
                    || !codes.insert(&h.code)
                    || !h.quantity.is_finite()
                    || h.quantity <= 0.0
                    || !h.cost_price.is_finite()
                {
                    bail!("账户 {} 的持仓无效或重复", account.id);
                }
                if let Some(q) = h.available_quantity {
                    if !q.is_finite() || q < 0.0 || q > h.quantity || h.available_date.is_none() {
                        bail!("账户 {} 的可用数量或日期无效", account.id);
                    }
                }
                if h.intraday_cost_price
                    .is_some_and(|p| !p.is_finite() || p <= 0.0)
                {
                    bail!("盘中成本无效");
                }
            }
        }
        let mut transactions = HashSet::new();
        for tx in &self.transactions {
            if tx.id.is_empty()
                || !transactions.insert(&tx.id)
                || !ids.contains(tx.account_id.as_str())
            {
                bail!("交易 ID 重复或引用未知账户");
            }
            if !tx.quantity.is_finite()
                || !tx.price.is_finite()
                || !tx.fees.is_finite()
                || tx.fees < 0.0
                || !tx.cash_amount.is_finite()
                || tx.realized_pnl.is_some_and(|v| !v.is_finite())
            {
                bail!("交易数值无效");
            }
            if matches!(tx.kind, TransactionKind::Buy | TransactionKind::Sell) {
                if tx.code.len() != 6
                    || !tx.code.chars().all(|c| c.is_ascii_digit())
                    || tx.quantity <= 0.0
                    || tx.price < 0.0
                {
                    bail!("买卖交易代码、数量或价格无效");
                }
                if tx.execution.price_basis == "fill" && tx.price <= 0.0 {
                    bail!("明确成交价必须为正数");
                }
            }
            if !["Unknown", "Inferred", "Confirmed", "Snapshot"]
                .contains(&tx.execution.confidence.as_str())
                || !["unknown", "estimated", "broker_cost", "fill"]
                    .contains(&tx.execution.price_basis.as_str())
            {
                bail!("交易证据类型无效");
            }
            if tx
                .execution
                .executed_at
                .is_some_and(|d| d.date_naive() != tx.date)
            {
                bail!("成交日期和时间不一致");
            }
        }
        for s in &self.snapshots {
            if ![
                s.total_assets,
                s.market_value,
                s.cash,
                s.position_pnl,
                s.today_pnl,
            ]
            .iter()
            .all(|n| n.is_finite())
            {
                bail!("资产快照包含无效金额");
            }
        }
        Ok(())
    }

    pub fn from_legacy_holdings(
        holdings: Vec<Holding>,
        last_saved_at: Option<DateTime<Local>>,
    ) -> Self {
        let mut portfolio = Self {
            loaded_from: None,
            accounts: vec![Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                owner: DEFAULT_OWNER.to_owned(),
                account_suffix: String::new(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings,
            }],
            transactions: Vec::new(),
            alerts: Vec::new(),
            watchlist: Vec::new(),
            snapshots: Vec::new(),
            last_saved_at,
        };
        portfolio.normalize();
        portfolio
    }

    pub fn holdings(&self) -> impl Iterator<Item = &Holding> {
        self.accounts
            .iter()
            .flat_map(|account| account.holdings.iter())
    }

    pub fn holdings_vec(&self) -> Vec<Holding> {
        self.holdings().cloned().collect()
    }

    pub fn quote_targets(&self) -> Vec<Holding> {
        let mut targets = self.holdings_vec();
        let mut seen = targets
            .iter()
            .map(|holding| holding.code.clone())
            .collect::<HashSet<_>>();

        for item in &self.watchlist {
            if seen.insert(item.code.clone()) {
                targets.push(quote_target(&item.code, &item.name, item.market));
            }
        }
        for alert in &self.alerts {
            if seen.insert(alert.code.clone()) {
                targets.push(quote_target(&alert.code, &alert.name, alert.market));
            }
        }
        for tx in self
            .transactions
            .iter()
            .filter(|tx| tx.kind == TransactionKind::Sell && tx.date == Local::now().date_naive())
        {
            if seen.insert(tx.code.clone()) {
                targets.push(quote_target(&tx.code, &tx.name, tx.market));
            }
        }
        if seen.insert("000300".to_owned()) {
            targets.push(quote_target("000300", "沪深300", Market::Shanghai));
        }
        targets
    }

    pub fn apply_transaction(&mut self, mut transaction: Transaction) -> anyhow::Result<()> {
        self.validate()?;
        if !transaction.id.is_empty() && self.transactions.iter().any(|t| t.id == transaction.id) {
            bail!("交易 ID 已存在，拒绝重复入账");
        }
        if !transaction.quantity.is_finite()
            || !transaction.price.is_finite()
            || !transaction.fees.is_finite()
            || transaction.fees < 0.0
            || !transaction.cash_amount.is_finite()
        {
            bail!("交易数值无效");
        }
        transaction.code = digits6(&transaction.code);
        transaction.name = transaction.name.trim().to_owned();
        transaction.quantity = finite_non_negative(transaction.quantity);
        transaction.price = finite_non_negative(transaction.price);
        transaction.fees = finite_non_negative(transaction.fees);
        transaction.cash_amount = if transaction.cash_amount.is_finite() {
            transaction.cash_amount
        } else {
            0.0
        };
        transaction.realized_pnl = None;
        transaction.execution = ExecutionEvidence {
            confidence: "Confirmed".into(),
            price_basis: "fill".into(),
            fees_known: true,
            date_is_estimated: false,
            ..transaction.execution
        };

        let account = self
            .accounts
            .iter_mut()
            .find(|account| account.id == transaction.account_id)
            .context("找不到交易账户")?;

        match transaction.kind {
            TransactionKind::Buy => {
                validate_security_transaction(&transaction)?;
                if account.cash + 0.000_001
                    < transaction.quantity * transaction.price + transaction.fees
                {
                    bail!("可用现金不足");
                }
                let today = Local::now().date_naive();
                let position = account
                    .holdings
                    .iter_mut()
                    .find(|holding| holding.code == transaction.code);
                if let Some(holding) = position {
                    let old_quantity = holding.quantity;
                    let available_before_buy = holding.overnight_quantity_for(transaction.date);
                    let old_intraday_quantity = holding.intraday_quantity_for(transaction.date);
                    let old_intraday_cost = holding.intraday_cost_price_for(transaction.date);
                    let new_quantity = old_quantity + transaction.quantity;
                    holding.cost_price = (holding.cost_price * old_quantity
                        + transaction.price * transaction.quantity
                        + transaction.fees)
                        / new_quantity;
                    holding.quantity = new_quantity;
                    holding.name = non_empty_name(&transaction.name, &holding.name);
                    holding.market = transaction.market;
                    if transaction.date == today {
                        holding.available_quantity = Some(available_before_buy);
                        holding.available_date = Some(transaction.date);
                        let intraday_quantity = old_intraday_quantity + transaction.quantity;
                        holding.intraday_cost_price = Some(
                            (old_intraday_cost * old_intraday_quantity
                                + transaction.price * transaction.quantity
                                + transaction.fees)
                                / intraday_quantity,
                        );
                    }
                } else {
                    account.holdings.push(Holding {
                        code: transaction.code.clone(),
                        name: non_empty_name(&transaction.name, &transaction.code),
                        quantity: transaction.quantity,
                        available_quantity: (transaction.date == today).then_some(0.0),
                        available_date: (transaction.date == today).then_some(transaction.date),
                        intraday_cost_price: (transaction.date == today)
                            .then_some(transaction.price + transaction.fees / transaction.quantity),
                        cost_price: transaction.price + transaction.fees / transaction.quantity,
                        market: transaction.market,
                    });
                }
                account.cash -= transaction.quantity * transaction.price + transaction.fees;
                transaction.cash_amount =
                    -(transaction.quantity * transaction.price + transaction.fees);
            }
            TransactionKind::Sell => {
                validate_security_transaction(&transaction)?;
                let holding_index = account
                    .holdings
                    .iter()
                    .position(|holding| holding.code == transaction.code)
                    .context("卖出股票不在当前持仓中")?;
                let holding = &mut account.holdings[holding_index];
                if transaction.quantity > holding.quantity + 0.000_001 {
                    bail!("卖出数量超过当前持仓");
                }
                if transaction.date == Local::now().date_naive()
                    && transaction.quantity
                        > holding.overnight_quantity_for(transaction.date) + 0.000_001
                {
                    bail!("卖出数量超过今日可用数量");
                }
                transaction.realized_pnl = Some(
                    (transaction.price - holding.cost_price) * transaction.quantity
                        - transaction.fees,
                );
                holding.quantity -= transaction.quantity;
                if holding.available_date == Some(transaction.date) {
                    holding.available_quantity = Some(
                        (holding.available_quantity.unwrap_or_default() - transaction.quantity)
                            .max(0.0),
                    );
                }
                account.cash += transaction.quantity * transaction.price - transaction.fees;
                transaction.cash_amount =
                    transaction.quantity * transaction.price - transaction.fees;
                if holding.quantity <= 0.000_001 {
                    account.holdings.remove(holding_index);
                }
            }
            TransactionKind::Dividend => {
                let net = transaction.cash_amount - transaction.fees;
                account.cash += net;
                transaction.realized_pnl = Some(net);
            }
            TransactionKind::Fee => {
                let amount = transaction.cash_amount.abs() + transaction.fees;
                account.cash -= amount;
                transaction.realized_pnl = Some(-amount);
            }
            TransactionKind::Deposit => account.cash += transaction.cash_amount.abs(),
            TransactionKind::Withdraw => account.cash -= transaction.cash_amount.abs(),
            TransactionKind::Adjustment => {
                account.cash += transaction.cash_amount;
            }
        }

        if transaction.id.trim().is_empty() {
            transaction.id = self.next_id("tx");
        }
        self.transactions.push(transaction);
        self.normalize();
        Ok(())
    }

    pub fn reconcile_account(
        &mut self,
        account_id: &str,
        imported: &[Holding],
        cash: Option<f64>,
        replace_missing: bool,
        date: NaiveDate,
    ) -> anyhow::Result<ReconcileSummary> {
        let account_index = self
            .accounts
            .iter()
            .position(|account| account.id == account_id)
            .context("找不到校准账户")?;
        let old = self.accounts[account_index]
            .holdings
            .iter()
            .map(|holding| (holding.code.clone(), holding.clone()))
            .collect::<HashMap<_, _>>();
        let incoming = imported
            .iter()
            .map(|holding| (holding.code.clone(), holding.clone()))
            .collect::<HashMap<_, _>>();
        let mut added = 0;
        let mut changed = 0;
        let mut removed = 0;
        let mut events = Vec::new();

        for holding in imported {
            match old.get(&holding.code) {
                None => {
                    added += 1;
                    events.push((holding.code.clone(), holding.name.clone(), holding.quantity));
                }
                Some(previous)
                    if (previous.quantity - holding.quantity).abs() > 0.000_001
                        || (previous.cost_price - holding.cost_price).abs() > 0.000_001 =>
                {
                    changed += 1;
                    events.push((
                        holding.code.clone(),
                        holding.name.clone(),
                        holding.quantity - previous.quantity,
                    ));
                }
                _ => {}
            }
        }
        if replace_missing {
            for previous in old.values() {
                if !incoming.contains_key(&previous.code) {
                    removed += 1;
                    events.push((
                        previous.code.clone(),
                        previous.name.clone(),
                        -previous.quantity,
                    ));
                }
            }
        }

        let account = &mut self.accounts[account_index];
        if replace_missing {
            account.holdings = imported.to_vec();
        } else {
            for holding in imported {
                if let Some(current) = account
                    .holdings
                    .iter_mut()
                    .find(|current| current.code == holding.code)
                {
                    *current = holding.clone();
                } else {
                    account.holdings.push(holding.clone());
                }
            }
        }
        if let Some(cash) = cash.filter(|cash| cash.is_finite()) {
            account.cash = cash;
        }

        for (code, name, quantity) in events {
            let price = incoming
                .get(&code)
                .map(|holding| holding.cost_price)
                .or_else(|| old.get(&code).map(|holding| holding.cost_price))
                .unwrap_or_default();
            let market = incoming
                .values()
                .chain(old.values())
                .find(|holding| holding.code == code)
                .map(|holding| holding.market)
                .unwrap_or_default();
            self.transactions.push(Transaction {
                id: self.next_id("adjust"),
                account_id: account_id.to_owned(),
                date,
                kind: TransactionKind::Adjustment,
                code,
                name,
                market,
                quantity,
                price,
                fees: 0.0,
                cash_amount: 0.0,
                realized_pnl: None,
                execution: ExecutionEvidence {
                    confidence: "Snapshot".into(),
                    ..Default::default()
                },
                note: "截图持仓校准".to_owned(),
            });
        }
        self.normalize();
        Ok(ReconcileSummary {
            added,
            changed,
            removed,
        })
    }

    pub fn upsert_snapshot(&mut self, snapshot: DailySnapshot) {
        if let Some(existing) = self
            .snapshots
            .iter_mut()
            .find(|existing| existing.date == snapshot.date)
        {
            *existing = snapshot;
        } else {
            self.snapshots.push(snapshot);
            self.snapshots.sort_by_key(|snapshot| snapshot.date);
        }
        if self.snapshots.len() > 730 {
            let remove_count = self.snapshots.len() - 730;
            self.snapshots.drain(0..remove_count);
        }
    }

    pub fn max_drawdown(&self) -> f64 {
        let mut peak = 0.0_f64;
        let mut max_drawdown = 0.0_f64;
        for snapshot in &self.snapshots {
            peak = peak.max(snapshot.total_assets);
            if peak > 0.0 {
                max_drawdown = max_drawdown.max((peak - snapshot.total_assets) / peak * 100.0);
            }
        }
        max_drawdown
    }

    pub fn next_id(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{}-{}",
            Local::now().timestamp_millis(),
            self.transactions.len() + self.alerts.len()
        )
    }

    pub fn normalize(&mut self) {
        self.ensure_account();
        for (idx, account) in self.accounts.iter_mut().enumerate() {
            account.id = sanitize_account_id(&account.id, idx);
            account.name = account.name.trim().to_owned();
            if account.name.is_empty() {
                account.name = format!("账户{}", idx + 1);
            }
            account.owner = account.owner.trim().to_owned();
            if account.owner.is_empty() {
                account.owner = DEFAULT_OWNER.to_owned();
            }
            account.account_suffix = account_suffix4(&account.account_suffix);
            if !account.cash.is_finite() {
                account.cash = 0.0;
            }
            if !account.today_realized_pnl.is_finite() {
                account.today_realized_pnl = 0.0;
            }
            if is_zero(&account.today_realized_pnl) {
                account.today_realized_pnl_date = None;
            }

            for holding in &mut account.holdings {
                holding.code = holding
                    .code
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .take(6)
                    .collect();
                if holding.code.len() == 6 {
                    holding.market = Market::infer(&holding.code);
                }
                holding.name = holding.name.trim().to_owned();
                holding.available_quantity = holding
                    .available_quantity
                    .filter(|quantity| quantity.is_finite())
                    .map(|quantity| quantity.clamp(0.0, holding.quantity));
                if holding.available_quantity.is_none() {
                    holding.available_date = None;
                }
                holding.intraday_cost_price = holding
                    .intraday_cost_price
                    .filter(|price| price.is_finite() && *price > 0.0);
            }
            account.holdings.retain(|h| {
                h.code.len() == 6
                    && h.quantity.is_finite()
                    && h.quantity > 0.0
                    && h.cost_price.is_finite()
            });
        }
        self.transactions.retain(|transaction| {
            !transaction.account_id.trim().is_empty()
                && transaction.quantity.is_finite()
                && transaction.price.is_finite()
                && transaction.fees.is_finite()
                && transaction.cash_amount.is_finite()
                && transaction.realized_pnl.is_none_or(f64::is_finite)
        });
        for transaction in &mut self.transactions {
            if transaction.execution.confidence != "Confirmed" {
                transaction.realized_pnl = None;
            }
        }
        for item in &mut self.watchlist {
            item.code = normalize_watch_code(&item.code);
            item.name = item.name.trim().to_owned();
            if item.code.len() == 6
                && item.code != "000300"
                && !(item.code == "000001" && item.name.contains("上证"))
            {
                item.market = Market::infer(&item.code);
            }
        }
        self.watchlist
            .retain(|item| item.code.len() == 6 || item.code == "KS11");
        self.watchlist.dedup_by(|a, b| a.code == b.code);

        for alert in &mut self.alerts {
            alert.code = digits6(&alert.code);
            alert.name = alert.name.trim().to_owned();
            if alert.code.len() == 6 && alert.code != "000300" {
                alert.market = Market::infer(&alert.code);
            }
            if !alert.threshold.is_finite() {
                alert.threshold = 0.0;
            }
            if !alert.high_watermark.is_finite() {
                alert.high_watermark = 0.0;
            }
        }
        self.alerts.retain(|alert| alert.code.len() == 6);
    }

    fn ensure_account(&mut self) {
        if self.accounts.is_empty() {
            self.accounts.push(Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                owner: DEFAULT_OWNER.to_owned(),
                account_suffix: String::new(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings: Vec::new(),
            });
        }
    }
}

impl Default for Market {
    fn default() -> Self {
        default_market()
    }
}

fn is_zero(value: &f64) -> bool {
    value.abs() < f64::EPSILON
}

fn default_true() -> bool {
    true
}

fn default_account_owner() -> String {
    DEFAULT_OWNER.to_owned()
}

fn is_default_owner(value: &String) -> bool {
    value.trim().is_empty() || value.trim() == DEFAULT_OWNER
}

fn digits6(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(6)
        .collect()
}

fn normalize_watch_code(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("KS11") {
        "KS11".to_owned()
    } else {
        digits6(value)
    }
}

fn account_suffix4(value: &str) -> String {
    let digits = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    digits
        .chars()
        .skip(digits.len().saturating_sub(4))
        .collect()
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn non_empty_name(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if primary.is_empty() {
        fallback.trim().to_owned()
    } else {
        primary.to_owned()
    }
}

fn validate_security_transaction(transaction: &Transaction) -> anyhow::Result<()> {
    if transaction.code.len() != 6 {
        bail!("股票代码必须是 6 位");
    }
    if transaction.quantity <= 0.0 {
        bail!("交易数量必须大于 0");
    }
    if transaction.price <= 0.0 {
        bail!("成交价格必须大于 0");
    }
    Ok(())
}

fn quote_target(code: &str, name: &str, market: Market) -> Holding {
    Holding {
        code: code.to_owned(),
        name: name.to_owned(),
        quantity: 1.0,
        available_quantity: None,
        available_date: None,
        intraday_cost_price: None,
        cost_price: 0.0,
        market,
    }
}

fn sanitize_account_id(id: &str, idx: usize) -> String {
    let clean = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect::<String>();
    if clean.is_empty() {
        format!("account-{}", idx + 1)
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_portfolio(cash: f64) -> Portfolio {
        Portfolio {
            loaded_from: None,
            accounts: vec![Account {
                id: "test".to_owned(),
                name: "测试账户".to_owned(),
                owner: DEFAULT_OWNER.to_owned(),
                account_suffix: String::new(),
                cash,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings: Vec::new(),
            }],
            transactions: Vec::new(),
            alerts: Vec::new(),
            watchlist: Vec::new(),
            snapshots: Vec::new(),
            last_saved_at: None,
        }
    }

    fn transaction(kind: TransactionKind, quantity: f64, price: f64) -> Transaction {
        Transaction {
            id: String::new(),
            account_id: "test".to_owned(),
            date: Local::now().date_naive(),
            kind,
            code: "600000".to_owned(),
            name: "浦发银行".to_owned(),
            market: Market::Shanghai,
            quantity,
            price,
            fees: 5.0,
            cash_amount: 0.0,
            realized_pnl: None,
            execution: ExecutionEvidence::default(),
            note: String::new(),
        }
    }

    #[test]
    fn normalize_defaults_blank_owner_to_me() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.accounts[0].owner.clear();
        portfolio.normalize();
        assert_eq!(portfolio.accounts[0].owner_label(), DEFAULT_OWNER);
    }

    #[test]
    fn normalize_keeps_last_four_account_digits() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.accounts[0].account_suffix = "王*540****9180".to_owned();
        portfolio.normalize();
        assert_eq!(portfolio.accounts[0].account_suffix, "9180");
    }

    #[test]
    fn normalize_keeps_kospi_watch_item() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.watchlist.push(WatchItem {
            code: "ks11".to_owned(),
            name: "韩国指数".to_owned(),
            market: Market::Shenzhen,
        });
        portfolio.normalize();
        assert_eq!(portfolio.watchlist[0].code, "KS11");
    }

    #[test]
    fn normalize_keeps_shanghai_market_for_shanghai_index() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.watchlist.push(WatchItem {
            code: "000001".to_owned(),
            name: "上证指数".to_owned(),
            market: Market::Shanghai,
        });
        portfolio.normalize();
        assert_eq!(portfolio.watchlist[0].market, Market::Shanghai);
    }

    #[test]
    fn buy_updates_cash_cost_and_today_available_quantity() {
        let mut portfolio = empty_portfolio(10_000.0);
        portfolio
            .apply_transaction(transaction(TransactionKind::Buy, 100.0, 10.0))
            .unwrap();
        let account = &portfolio.accounts[0];
        let holding = &account.holdings[0];
        assert!((account.cash - 8_995.0).abs() < 0.001);
        assert!((holding.cost_price - 10.05).abs() < 0.001);
        assert_eq!(holding.available_quantity, Some(0.0));
        assert_eq!(holding.available_date, Some(Local::now().date_naive()));
    }

    #[test]
    fn repeated_buys_weight_all_intraday_lots_and_reject_duplicate_id() {
        let mut p = empty_portfolio(100_000.0);
        let mut t = transaction(TransactionKind::Buy, 100.0, 10.0);
        t.id = "one".into();
        p.apply_transaction(t.clone()).unwrap();
        assert!(p.apply_transaction(t).is_err());
        p.apply_transaction(transaction(TransactionKind::Buy, 200.0, 13.0))
            .unwrap();
        let h = &p.accounts[0].holdings[0];
        assert_eq!(h.quantity, 300.0);
        assert!((h.intraday_cost_price.unwrap() - 3610.0 / 300.0).abs() < 1e-8);
        assert_eq!(h.overnight_quantity_for(Local::now().date_naive()), 0.0);
        assert_eq!(
            h.overnight_quantity_for(Local::now().date_naive().succ_opt().unwrap()),
            300.0
        );
        p.accounts[0].holdings[0].available_quantity = Some(400.0);
        assert!(p.validate().is_err());
    }

    #[test]
    fn daily_sell_pnl_is_measured_from_previous_close_and_unknown_stays_unknown() {
        let mut t = transaction(TransactionKind::Sell, 100.0, 14.0);
        t.realized_pnl = Some(395.0);
        assert_eq!(t.daily_pnl(Some(15.0)), None);
        t.execution = ExecutionEvidence {
            confidence: "Confirmed".into(),
            price_basis: "fill".into(),
            fees_known: true,
            date_is_estimated: false,
            ..Default::default()
        };
        assert_eq!(t.daily_pnl(Some(15.0)), Some(-105.0));
        assert_eq!(t.daily_pnl(None), None);
        assert!(Portfolio::default().holdings().next().is_none());
    }

    #[test]
    fn sell_records_realized_pnl_and_cash() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.accounts[0].holdings.push(Holding {
            code: "600000".to_owned(),
            name: "浦发银行".to_owned(),
            quantity: 200.0,
            available_quantity: None,
            available_date: None,
            intraday_cost_price: None,
            cost_price: 8.0,
            market: Market::Shanghai,
        });
        portfolio
            .apply_transaction(transaction(TransactionKind::Sell, 100.0, 10.0))
            .unwrap();
        assert!((portfolio.accounts[0].cash - 995.0).abs() < 0.001);
        assert_eq!(portfolio.accounts[0].holdings[0].quantity, 100.0);
        assert!((portfolio.transactions[0].realized_pnl.unwrap() - 195.0).abs() < 0.001);
    }

    #[test]
    fn reconcile_can_preserve_or_remove_missing_holdings() {
        let mut portfolio = empty_portfolio(0.0);
        portfolio.accounts[0].holdings.push(Holding {
            code: "600000".to_owned(),
            name: "旧持仓".to_owned(),
            quantity: 100.0,
            available_quantity: None,
            available_date: None,
            intraday_cost_price: None,
            cost_price: 10.0,
            market: Market::Shanghai,
        });
        let imported = vec![Holding {
            code: "000001".to_owned(),
            name: "新持仓".to_owned(),
            quantity: 200.0,
            available_quantity: None,
            available_date: None,
            intraday_cost_price: None,
            cost_price: 12.0,
            market: Market::Shenzhen,
        }];
        portfolio
            .reconcile_account(
                "test",
                &imported,
                Some(123.0),
                false,
                Local::now().date_naive(),
            )
            .unwrap();
        assert_eq!(portfolio.accounts[0].holdings.len(), 2);
        portfolio
            .reconcile_account(
                "test",
                &imported,
                Some(456.0),
                true,
                Local::now().date_naive(),
            )
            .unwrap();
        assert_eq!(portfolio.accounts[0].holdings.len(), 1);
        assert_eq!(portfolio.accounts[0].cash, 456.0);
    }
}
