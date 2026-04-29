use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Holding {
    pub code: String,
    pub name: String,
    pub quantity: f64,
    pub cost_price: f64,
    #[serde(default = "default_market")]
    pub market: Market,
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
    pub holdings: Vec<Holding>,
    #[serde(default)]
    pub last_saved_at: Option<DateTime<Local>>,
}

impl Default for Portfolio {
    fn default() -> Self {
        Self {
            holdings: vec![
                Holding {
                    code: "600519".to_owned(),
                    name: "贵州茅台".to_owned(),
                    quantity: 100.0,
                    cost_price: 1500.0,
                    market: Market::Shanghai,
                },
                Holding {
                    code: "000001".to_owned(),
                    name: "平安银行".to_owned(),
                    quantity: 1000.0,
                    cost_price: 10.0,
                    market: Market::Shenzhen,
                },
            ],
            last_saved_at: None,
        }
    }
}

impl Portfolio {
    pub fn normalize(&mut self) {
        for holding in &mut self.holdings {
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
        }
        self.holdings.retain(|h| {
            h.code.len() == 6
                && h.quantity.is_finite()
                && h.quantity > 0.0
                && h.cost_price.is_finite()
        });
    }
}
