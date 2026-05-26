use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Holding {
    pub code: String,
    pub name: String,
    pub quantity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_quantity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_date: Option<NaiveDate>,
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
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub last_saved_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub cash: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub today_realized_pnl: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today_realized_pnl_date: Option<NaiveDate>,
    pub holdings: Vec<Holding>,
}

impl Account {
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
            accounts: vec![Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings: vec![
                    Holding {
                        code: "600519".to_owned(),
                        name: "贵州茅台".to_owned(),
                        quantity: 100.0,
                        available_quantity: None,
                        available_date: None,
                        cost_price: 1500.0,
                        market: Market::Shanghai,
                    },
                    Holding {
                        code: "000001".to_owned(),
                        name: "平安银行".to_owned(),
                        quantity: 1000.0,
                        available_quantity: None,
                        available_date: None,
                        cost_price: 10.0,
                        market: Market::Shenzhen,
                    },
                ],
            }],
            last_saved_at: None,
        }
    }
}

impl Portfolio {
    pub fn from_legacy_holdings(
        holdings: Vec<Holding>,
        last_saved_at: Option<DateTime<Local>>,
    ) -> Self {
        let mut portfolio = Self {
            accounts: vec![Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings,
            }],
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

    pub fn cash(&self) -> f64 {
        self.accounts
            .iter()
            .map(|account| account.cash)
            .filter(|cash| cash.is_finite())
            .sum()
    }

    pub fn today_realized_pnl_for(&self, quote_date: NaiveDate) -> f64 {
        self.accounts
            .iter()
            .map(|account| account.today_realized_pnl_for(quote_date))
            .filter(|pnl| pnl.is_finite())
            .sum()
    }

    pub fn push_holding_to_first_account(&mut self, holding: Holding) {
        self.ensure_account();
        self.accounts[0].holdings.push(holding);
    }

    pub fn append_holdings_to_first_account(&mut self, holdings: &mut Vec<Holding>) {
        self.ensure_account();
        self.accounts[0].holdings.append(holdings);
    }

    pub fn normalize(&mut self) {
        self.ensure_account();
        for (idx, account) in self.accounts.iter_mut().enumerate() {
            account.id = sanitize_account_id(&account.id, idx);
            account.name = account.name.trim().to_owned();
            if account.name.is_empty() {
                account.name = format!("账户{}", idx + 1);
            }
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
            }
            account.holdings.retain(|h| {
                h.code.len() == 6
                    && h.quantity.is_finite()
                    && h.quantity > 0.0
                    && h.cost_price.is_finite()
            });
        }
    }

    fn ensure_account(&mut self) {
        if self.accounts.is_empty() {
            self.accounts.push(Account {
                id: "default".to_owned(),
                name: "默认账户".to_owned(),
                cash: 0.0,
                today_realized_pnl: 0.0,
                today_realized_pnl_date: None,
                holdings: Vec::new(),
            });
        }
    }
}

fn is_zero(value: &f64) -> bool {
    value.abs() < f64::EPSILON
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
