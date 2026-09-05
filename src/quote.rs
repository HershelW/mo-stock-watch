use crate::portfolio::{Holding, Market};
use anyhow::{bail, Context};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use encoding_rs::GBK;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub code: String,
    pub name: String,
    pub price: f64,
    pub previous_close: f64,
    pub change_percent: f64,
    pub updated_at: DateTime<Local>,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub turnover_rate: Option<f64>,
    #[serde(default)]
    pub pe_ratio: Option<f64>,
    #[serde(default)]
    pub market_cap: Option<f64>,
}

impl Quote {
    pub fn position_pnl(&self, holding: &Holding) -> f64 {
        (self.price - holding.cost_price) * holding.quantity
    }

    pub fn today_pnl(&self, holding: &Holding) -> f64 {
        let quote_date = self.updated_at.date_naive();
        let overnight_quantity = holding.overnight_quantity_for(quote_date);
        let intraday_quantity = holding.intraday_quantity_for(quote_date);
        let intraday_cost_price = holding.intraday_cost_price_for(quote_date);

        (self.price - self.previous_close) * overnight_quantity
            + (self.price - intraday_cost_price) * intraday_quantity
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

pub fn load_cache() -> QuoteBook {
    let path = crate::config::app_dir().join("quote_cache.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return QuoteBook::default();
    };
    let Ok(quotes) = serde_json::from_str::<Vec<Quote>>(&raw) else {
        return QuoteBook::default();
    };
    let last_updated_at = quotes.iter().map(|quote| quote.updated_at).min();
    QuoteBook {
        quotes: quotes
            .into_iter()
            .map(|quote| (quote.code.clone(), quote))
            .collect(),
        last_updated_at,
        last_error: None,
        loading: false,
    }
}

pub fn save_cache(quotes: &HashMap<String, Quote>) -> anyhow::Result<()> {
    fs::create_dir_all(crate::config::app_dir()).context("create quote cache dir")?;
    let values = quotes.values().cloned().collect::<Vec<_>>();
    fs::write(
        crate::config::app_dir().join("quote_cache.json"),
        serde_json::to_string(&values).context("serialize quote cache")?,
    )
    .context("write quote cache")
}

fn fetch_quotes_with_fallback(holdings: &[Holding]) -> anyhow::Result<QuoteFetchResult> {
    let mut errors = Vec::new();
    let mut collected = HashMap::new();

    for (source, fetcher) in [
        (
            QuoteSource::EastMoney,
            fetch_eastmoney_quotes as fn(&[Holding]) -> anyhow::Result<Vec<Quote>>,
        ),
        (QuoteSource::Tencent, fetch_tencent_quotes),
        (QuoteSource::Sina, fetch_sina_quotes),
    ] {
        match fetcher(holdings) {
            Ok(mut quotes) if !quotes.is_empty() || holdings.is_empty() => {
                if holdings.iter().any(|holding| holding.code == "KS11")
                    && !quotes.iter().any(|quote| quote.code == "KS11")
                {
                    let global_targets = holdings
                        .iter()
                        .filter(|holding| holding.code == "KS11")
                        .cloned()
                        .collect::<Vec<_>>();
                    if !global_targets.is_empty() {
                        if let Ok(global_quotes) = fetch_eastmoney_quotes(&global_targets) {
                            if !global_quotes.is_empty() {
                                quotes.extend(global_quotes);
                            } else if let Ok(kospi) = fetch_yahoo_kospi() {
                                quotes.push(kospi);
                            }
                        } else if let Ok(kospi) = fetch_yahoo_kospi() {
                            quotes.push(kospi);
                        }
                    }
                }
                for quote in quotes {
                    if quote.price.is_finite()
                        && quote.price > 0.0
                        && quote.previous_close.is_finite()
                        && quote.previous_close > 0.0
                    {
                        collected.entry(quote.code.clone()).or_insert(quote);
                    }
                }
                let merged = collected.values().cloned().collect::<Vec<_>>();
                if quotes_cover_targets(holdings, &merged) {
                    return Ok(QuoteFetchResult {
                        quotes: merged,
                        source,
                    });
                }
                errors.push(format!("{}: 部分持仓缺少行情", source.label()));
            }
            Ok(_) => errors.push(format!("{}: empty quotes", source.label())),
            Err(err) => errors.push(format!("{}: {err:#}", source.label())),
        }
    }

    bail!("全部行情源失败：{}", errors.join(" | "))
}

fn quotes_cover_targets(holdings: &[Holding], quotes: &[Quote]) -> bool {
    holdings.iter().all(|h| {
        quotes.iter().any(|q| {
            q.code == h.code
                && q.price.is_finite()
                && q.price > 0.0
                && q.previous_close.is_finite()
                && q.previous_close > 0.0
        })
    })
}

fn secid(market: Market, code: &str, name: &str) -> String {
    if code == "KS11" {
        return "100.KS11".to_owned();
    }
    if code == "000001" && name.contains("上证") {
        return "1.000001".to_owned();
    }
    format!("{}.{}", market.eastmoney_prefix(), code)
}

fn fetch_eastmoney_quotes(holdings: &[Holding]) -> anyhow::Result<Vec<Quote>> {
    let secids = holdings
        .iter()
        .filter(|h| h.code.len() == 6 || h.code == "KS11")
        .map(|h| secid(h.market, &h.code, &h.name))
        .collect::<Vec<_>>()
        .join(",");

    if secids.is_empty() {
        return Ok(Vec::new());
    }

    let url = "https://push2.eastmoney.com/api/qt/ulist.np/get";
    let response: EastMoneyResponse = quote_client()
        .get(url)
        .header("Referer", "https://quote.eastmoney.com/")
        .query(&[
            ("fltt", "2"),
            ("invt", "2"),
            (
                "fields",
                "f12,f14,f2,f3,f4,f5,f6,f8,f9,f15,f16,f17,f18,f20,f124",
            ),
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
                updated_at: DateTime::from_timestamp(row.timestamp.unwrap_or(0), 0)
                    .unwrap_or_default()
                    .with_timezone(&Local),
                open: row.open,
                high: row.high,
                low: row.low,
                volume: row.volume,
                amount: row.amount,
                turnover_rate: row.turnover_rate,
                pe_ratio: row.pe_ratio,
                market_cap: row.market_cap,
            })
        })
        .collect())
}

fn fetch_yahoo_kospi() -> anyhow::Result<Quote> {
    let response: YahooChartResponse = quote_client()
        .get("https://query1.finance.yahoo.com/v8/finance/chart/%5EKS11")
        .query(&[("range", "1d"), ("interval", "1m")])
        .send()
        .context("request yahoo kospi")?
        .error_for_status()
        .context("yahoo kospi http status")?
        .json()
        .context("parse yahoo kospi response")?;
    let meta = response
        .chart
        .result
        .into_iter()
        .next()
        .context("yahoo kospi missing result")?
        .meta;
    let price = meta.price.context("yahoo kospi missing price")?;
    let previous_close = meta
        .previous_close
        .context("yahoo kospi missing previous close")?;
    Ok(Quote {
        code: "KS11".to_owned(),
        name: "韩国指数".to_owned(),
        price,
        previous_close,
        change_percent: (price - previous_close) / previous_close * 100.0,
        updated_at: Local::now(),
        open: None,
        high: None,
        low: None,
        volume: None,
        amount: None,
        turnover_rate: None,
        pe_ratio: None,
        market_cap: None,
    })
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

    let bytes = quote_client()
        .get("https://qt.gtimg.cn/q")
        .header("Referer", "https://gu.qq.com/")
        .query(&[("q", symbols.as_str())])
        .send()
        .context("request tencent quotes")?
        .error_for_status()
        .context("tencent http status")?
        .bytes()
        .context("read tencent response")?;
    let text = decode_gbk(&bytes);

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

    let bytes = quote_client()
        .get("https://hq.sinajs.cn/list=".to_owned() + &symbols)
        .header("Referer", "https://finance.sina.com.cn/")
        .send()
        .context("request sina quotes")?
        .error_for_status()
        .context("sina http status")?
        .bytes()
        .context("read sina response")?;
    let text = decode_gbk(&bytes);

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

fn parse_tencent_quote_line(line: &str, _received_at: DateTime<Local>) -> Option<Quote> {
    let content = line.split_once('"')?.1.rsplit_once('"')?.0;
    let fields = content.split('~').collect::<Vec<_>>();
    let updated_at = provider_time(fields.get(30).copied().unwrap_or(""), "%Y%m%d%H%M%S");
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
        open: fields.get(5).and_then(|value| parse_f64(value)),
        high: fields.get(33).and_then(|value| parse_f64(value)),
        low: fields.get(34).and_then(|value| parse_f64(value)),
        volume: fields.get(6).and_then(|value| parse_f64(value)),
        amount: fields.get(37).and_then(|value| parse_f64(value)),
        turnover_rate: fields.get(38).and_then(|value| parse_f64(value)),
        pe_ratio: fields.get(39).and_then(|value| parse_f64(value)),
        market_cap: fields.get(45).and_then(|value| parse_f64(value)),
    })
}

fn parse_sina_quote_line(line: &str, _received_at: DateTime<Local>) -> Option<Quote> {
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
    let updated_at = provider_time(
        &format!(
            "{} {}",
            fields.get(30).unwrap_or(&""),
            fields.get(31).unwrap_or(&"")
        ),
        "%Y-%m-%d %H:%M:%S",
    );
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
        open: fields.get(1).and_then(|value| parse_f64(value)),
        high: fields.get(4).and_then(|value| parse_f64(value)),
        low: fields.get(5).and_then(|value| parse_f64(value)),
        volume: fields.get(8).and_then(|value| parse_f64(value)),
        amount: fields.get(9).and_then(|value| parse_f64(value)),
        turnover_rate: None,
        pe_ratio: None,
        market_cap: None,
    })
}

fn provider_time(value: &str, format: &str) -> DateTime<Local> {
    NaiveDateTime::parse_from_str(value, format)
        .ok()
        .and_then(|t| {
            FixedOffset::east_opt(8 * 3600)?
                .from_local_datetime(&t)
                .single()
        })
        .map(|t| t.with_timezone(&Local))
        .unwrap_or_else(|| DateTime::UNIX_EPOCH.with_timezone(&Local))
}

fn parse_f64(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn decode_gbk(bytes: &[u8]) -> String {
    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

fn quote_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126.0 Safari/537.36")
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
    #[serde(rename = "f124", default)]
    timestamp: Option<i64>,
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
    #[serde(rename = "f17")]
    open: Option<f64>,
    #[serde(rename = "f15")]
    high: Option<f64>,
    #[serde(rename = "f16")]
    low: Option<f64>,
    #[serde(rename = "f5")]
    volume: Option<f64>,
    #[serde(rename = "f6")]
    amount: Option<f64>,
    #[serde(rename = "f8")]
    turnover_rate: Option<f64>,
    #[serde(rename = "f9")]
    pe_ratio: Option<f64>,
    #[serde(rename = "f20")]
    market_cap: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct YahooChartResponse {
    chart: YahooChart,
}

#[derive(Debug, Deserialize)]
struct YahooChart {
    result: Vec<YahooChartResult>,
}

#[derive(Debug, Deserialize)]
struct YahooChartResult {
    meta: YahooChartMeta,
}

#[derive(Debug, Deserialize)]
struct YahooChartMeta {
    #[serde(rename = "regularMarketPrice")]
    price: Option<f64>,
    #[serde(rename = "previousClose")]
    previous_close: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_or_invalid_quotes_do_not_cover_a_portfolio() {
        let holding: Holding = serde_json::from_value(serde_json::json!({
            "code":"600000", "name":"fixture", "quantity":100, "cost_price":10
        }))
        .unwrap();
        let quote: Quote = serde_json::from_value(serde_json::json!({
            "code":"600000", "name":"fixture", "price":10, "previous_close":9,
            "change_percent":11.1, "updated_at":Local::now()
        }))
        .unwrap();
        assert!(quotes_cover_targets(&[holding.clone()], &[quote.clone()]));
        assert!(!quotes_cover_targets(&[holding.clone()], &[]));
        let mut bad = quote;
        bad.price = f64::NAN;
        assert!(!quotes_cover_targets(&[holding], &[bad]));
    }
    use chrono::TimeZone;

    #[test]
    fn maps_kospi_to_eastmoney_global_secid() {
        assert_eq!(secid(Market::Shenzhen, "KS11", "韩国指数"), "100.KS11");
        assert_eq!(secid(Market::Shenzhen, "000001", "上证指数"), "1.000001");
    }

    #[test]
    fn parses_yahoo_kospi_meta() {
        let response: YahooChartResponse = serde_json::from_str(
            r#"{"chart":{"result":[{"meta":{"regularMarketPrice":5639.18,"previousClose":6023.66}}]}}"#,
        )
        .expect("yahoo response");
        let meta = &response.chart.result[0].meta;
        assert_eq!(meta.price, Some(5639.18));
        assert_eq!(meta.previous_close, Some(6023.66));
    }

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
        assert_eq!(
            quote.updated_at,
            provider_time("20260429103548", "%Y%m%d%H%M%S")
        );
        assert_eq!(provider_time("", "%Y%m%d%H%M%S").timestamp(), 0);
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

    #[test]
    fn decodes_gbk_quote_names() {
        let (bytes, _, _) = GBK.encode("国际复材~日联科技~胜宏科技");
        assert_eq!(decode_gbk(&bytes), "国际复材~日联科技~胜宏科技");
    }

    #[test]
    fn today_pnl_uses_cost_for_same_day_unavailable_shares() {
        let now = Local.with_ymd_and_hms(2026, 5, 20, 11, 27, 0).unwrap();
        let quote = Quote {
            code: "603986".to_owned(),
            name: "兆易创新".to_owned(),
            price: 439.56,
            previous_close: 410.0,
            change_percent: 7.21,
            updated_at: now,
            open: None,
            high: None,
            low: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            pe_ratio: None,
            market_cap: None,
        };
        let holding = Holding {
            code: "603986".to_owned(),
            name: "兆易创新".to_owned(),
            quantity: 300.0,
            available_quantity: Some(0.0),
            available_date: Some(now.date_naive()),
            intraday_cost_price: None,
            cost_price: 423.538,
            market: Market::Shanghai,
        };

        assert!((quote.today_pnl(&holding) - 4806.6).abs() < 0.01);
    }

    #[test]
    fn today_pnl_treats_old_available_date_as_overnight() {
        let now = Local.with_ymd_and_hms(2026, 5, 21, 9, 35, 0).unwrap();
        let quote = Quote {
            code: "603986".to_owned(),
            name: "兆易创新".to_owned(),
            price: 439.56,
            previous_close: 430.0,
            change_percent: 2.22,
            updated_at: now,
            open: None,
            high: None,
            low: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            pe_ratio: None,
            market_cap: None,
        };
        let holding = Holding {
            code: "603986".to_owned(),
            name: "兆易创新".to_owned(),
            quantity: 300.0,
            available_quantity: Some(0.0),
            available_date: Some(
                Local
                    .with_ymd_and_hms(2026, 5, 20, 11, 27, 0)
                    .unwrap()
                    .date_naive(),
            ),
            intraday_cost_price: None,
            cost_price: 423.538,
            market: Market::Shanghai,
        };

        assert!((quote.today_pnl(&holding) - 2868.0).abs() < 0.01);
    }

    #[test]
    fn today_pnl_uses_intraday_cost_for_same_day_buys() {
        let now = Local.with_ymd_and_hms(2026, 5, 26, 11, 11, 0).unwrap();
        let quote = Quote {
            code: "688531".to_owned(),
            name: "日联科技".to_owned(),
            price: 160.84,
            previous_close: 172.98,
            change_percent: -7.02,
            updated_at: now,
            open: None,
            high: None,
            low: None,
            volume: None,
            amount: None,
            turnover_rate: None,
            pe_ratio: None,
            market_cap: None,
        };
        let holding = Holding {
            code: "688531".to_owned(),
            name: "日联科技".to_owned(),
            quantity: 800.0,
            available_quantity: Some(599.0),
            available_date: Some(now.date_naive()),
            intraday_cost_price: Some(161.54),
            cost_price: 137.63,
            market: Market::Shanghai,
        };

        assert!((quote.today_pnl(&holding) + 7412.56).abs() < 0.01);
    }
}
