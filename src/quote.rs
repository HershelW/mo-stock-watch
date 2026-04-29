use crate::portfolio::{Holding, Market};
use anyhow::{bail, Context};
use chrono::{DateTime, Local};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Default)]
pub struct QuoteBook {
    pub quotes: HashMap<String, Quote>,
    pub last_updated_at: Option<DateTime<Local>>,
    pub last_error: Option<String>,
    pub loading: bool,
}

#[derive(Debug, Clone)]
pub struct QuoteFetchResult {
    pub quotes: Vec<Quote>,
    pub source: QuoteSource,
}

#[derive(Debug, Clone, Copy)]
pub enum QuoteSource {
    EastMoney,
    Tencent,
    Sina,
}

impl QuoteSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::EastMoney => "东方财富",
            Self::Tencent => "腾讯",
            Self::Sina => "新浪",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub code: String,
    pub name: String,
    pub price: f64,
    pub previous_close: f64,
    pub change_percent: f64,
    pub updated_at: DateTime<Local>,
}

impl Quote {
    pub fn position_pnl(&self, holding: &Holding) -> f64 {
        (self.price - holding.cost_price) * holding.quantity
    }

    pub fn today_pnl(&self, holding: &Holding) -> f64 {
        (self.price - self.previous_close) * holding.quantity
    }

    pub fn market_value(&self, holding: &Holding) -> f64 {
        self.price * holding.quantity
    }
}

pub fn spawn_fetch(holdings: Vec<Holding>) -> Receiver<anyhow::Result<QuoteFetchResult>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = fetch_quotes_with_fallback(&holdings);
        let _ = tx.send(result);
    });
    rx
}

fn fetch_quotes_with_fallback(holdings: &[Holding]) -> anyhow::Result<QuoteFetchResult> {
    let mut errors = Vec::new();

    for (source, fetcher) in [
        (
            QuoteSource::EastMoney,
            fetch_eastmoney_quotes as fn(&[Holding]) -> anyhow::Result<Vec<Quote>>,
        ),
        (QuoteSource::Tencent, fetch_tencent_quotes),
        (QuoteSource::Sina, fetch_sina_quotes),
    ] {
        match fetcher(holdings) {
            Ok(quotes) if !quotes.is_empty() || holdings.is_empty() => {
                return Ok(QuoteFetchResult { quotes, source });
            }
            Ok(_) => errors.push(format!("{}: empty quotes", source.label())),
            Err(err) => errors.push(format!("{}: {err:#}", source.label())),
        }
    }

    bail!("全部行情源失败：{}", errors.join(" | "))
}

fn secid(market: Market, code: &str) -> String {
    format!("{}.{}", market.eastmoney_prefix(), code)
}

fn fetch_eastmoney_quotes(holdings: &[Holding]) -> anyhow::Result<Vec<Quote>> {
    let secids = holdings
        .iter()
        .filter(|h| h.code.len() == 6)
        .map(|h| secid(h.market, &h.code))
        .collect::<Vec<_>>()
        .join(",");

    if secids.is_empty() {
        return Ok(Vec::new());
    }

    let url = "https://push2.eastmoney.com/api/qt/ulist.np/get";
    let response: EastMoneyResponse = quote_client()
        .get(url)
        .query(&[
            ("fltt", "2"),
            ("invt", "2"),
            ("fields", "f12,f14,f2,f3,f4,f18"),
            ("secids", secids.as_str()),
        ])
        .send()
        .context("request eastmoney quotes")?
        .error_for_status()
        .context("eastmoney http status")?
        .json()
        .context("parse eastmoney response")?;

    let Some(data) = response.data else {
        bail!("行情源没有返回数据");
    };

    let now = Local::now();
    Ok(data
        .diff
        .into_iter()
        .filter_map(|row| {
            let price = row.price?;
            let previous_close = row.previous_close?;
            Some(Quote {
                code: row.code,
                name: row.name,
                price,
                previous_close,
                change_percent: row.change_percent.unwrap_or_default(),
                updated_at: now,
            })
        })
        .collect())
}

fn fetch_tencent_quotes(holdings: &[Holding]) -> anyhow::Result<Vec<Quote>> {
    let symbols = holdings
        .iter()
        .filter(|h| h.code.len() == 6)
        .map(|h| prefixed_symbol(h.market, &h.code))
        .collect::<Vec<_>>()
        .join(",");

    if symbols.is_empty() {
        return Ok(Vec::new());
    }

    let text = quote_client()
        .get("https://qt.gtimg.cn/q")
        .header("Referer", "https://gu.qq.com/")
        .query(&[("q", symbols.as_str())])
        .send()
        .context("request tencent quotes")?
        .error_for_status()
        .context("tencent http status")?
        .text()
        .context("read tencent response")?;

    let now = Local::now();
    Ok(text
        .lines()
        .filter_map(|line| parse_tencent_quote_line(line, now))
        .collect())
}

fn fetch_sina_quotes(holdings: &[Holding]) -> anyhow::Result<Vec<Quote>> {
    let symbols = holdings
        .iter()
        .filter(|h| h.code.len() == 6)
        .map(|h| prefixed_symbol(h.market, &h.code))
        .collect::<Vec<_>>()
        .join(",");

    if symbols.is_empty() {
        return Ok(Vec::new());
    }

    let text = quote_client()
        .get("https://hq.sinajs.cn/list=".to_owned() + &symbols)
        .header("Referer", "https://finance.sina.com.cn/")
        .send()
        .context("request sina quotes")?
        .error_for_status()
        .context("sina http status")?
        .text()
        .context("read sina response")?;

    let now = Local::now();
    Ok(text
        .lines()
        .filter_map(|line| parse_sina_quote_line(line, now))
        .collect())
}

fn prefixed_symbol(market: Market, code: &str) -> String {
    let prefix = match market {
        Market::Shanghai => "sh",
        Market::Shenzhen => "sz",
        Market::Beijing => "bj",
    };
    format!("{prefix}{code}")
}

fn parse_tencent_quote_line(line: &str, updated_at: DateTime<Local>) -> Option<Quote> {
    let content = line.split_once('"')?.1.rsplit_once('"')?.0;
    let fields = content.split('~').collect::<Vec<_>>();
    let name = fields.get(1)?.to_string();
    let code = fields.get(2)?.to_string();
    let price = parse_f64(fields.get(3)?)?;
    let previous_close = parse_f64(fields.get(4)?)?;
    if price <= 0.0 || previous_close <= 0.0 {
        return None;
    }
    let change_percent = fields
        .get(32)
        .and_then(|value| parse_f64(value))
        .unwrap_or_else(|| (price - previous_close) / previous_close * 100.0);

    Some(Quote {
        code,
        name,
        price,
        previous_close,
        change_percent,
        updated_at,
    })
}

fn parse_sina_quote_line(line: &str, updated_at: DateTime<Local>) -> Option<Quote> {
    let var_name = line.split_once('=')?.0;
    let code = var_name
        .chars()
        .rev()
        .take(6)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if code.len() != 6 {
        return None;
    }

    let content = line.split_once('"')?.1.rsplit_once('"')?.0;
    let fields = content.split(',').collect::<Vec<_>>();
    let name = fields.first()?.to_string();
    let previous_close = parse_f64(fields.get(2)?)?;
    let price = parse_f64(fields.get(3)?)?;
    if price <= 0.0 || previous_close <= 0.0 {
        return None;
    }
    let change_percent = (price - previous_close) / previous_close * 100.0;

    Some(Quote {
        code,
        name,
        price,
        previous_close,
        change_percent,
        updated_at,
    })
}

fn parse_f64(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn quote_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 mo-stock-watch/0.1")
        .timeout(Duration::from_secs(5))
        .build()
        .expect("valid quote http client")
}

#[derive(Debug, Deserialize)]
struct EastMoneyResponse {
    data: Option<EastMoneyData>,
}

#[derive(Debug, Deserialize)]
struct EastMoneyData {
    diff: Vec<EastMoneyQuoteRow>,
}

#[derive(Debug, Deserialize)]
struct EastMoneyQuoteRow {
    #[serde(rename = "f12")]
    code: String,
    #[serde(rename = "f14")]
    name: String,
    #[serde(rename = "f2")]
    price: Option<f64>,
    #[serde(rename = "f3")]
    change_percent: Option<f64>,
    #[serde(rename = "f18")]
    previous_close: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_tencent_quote_line() {
        let now = Local.with_ymd_and_hms(2026, 4, 29, 10, 35, 48).unwrap();
        let line = "v_sh600519=\"1~贵州茅台~600519~1501.00~1500.00~1498.00~57319~28788~28492~1500.50~5~1500.00~14~1499.50~3~1499.00~10~1498.50~2~1501.00~1~1501.50~15~1502.00~1~1502.50~13~1503.00~86~~20260429103548~1.00~0.07~1510.00~1490.00\";";

        let quote = parse_tencent_quote_line(line, now).expect("quote");

        assert_eq!(quote.code, "600519");
        assert_eq!(quote.name, "贵州茅台");
        assert_eq!(quote.price, 1501.00);
        assert_eq!(quote.previous_close, 1500.00);
        assert_eq!(quote.change_percent, 0.07);
    }

    #[test]
    fn parses_sina_quote_line() {
        let now = Local.with_ymd_and_hms(2026, 4, 29, 10, 35, 48).unwrap();
        let line = "var hq_str_sh600519=\"贵州茅台,1498.000,1500.000,1501.000,1510.000,1490.000,1500.500,1501.500,5732446,2832984337.010\";";

        let quote = parse_sina_quote_line(line, now).expect("quote");

        assert_eq!(quote.code, "600519");
        assert_eq!(quote.name, "贵州茅台");
        assert_eq!(quote.price, 1501.00);
        assert_eq!(quote.previous_close, 1500.00);
        assert!((quote.change_percent - 0.0667).abs() < 0.001);
    }
}
