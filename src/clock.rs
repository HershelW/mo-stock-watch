use anyhow::{bail, Context};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest::header::{HeaderMap, AGE, CACHE_CONTROL, DATE};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Once, OnceLock, RwLock,
    },
    time::{Duration, Instant},
};

pub const BEIJING: FixedOffset = FixedOffset::east_opt(8 * 3600).unwrap();
const SOURCES: [&str; 2] = [
    "https://qt.gtimg.cn/q=sh000001",
    "https://push2.eastmoney.com/api/qt/stock/get?secid=1.000001&fields=f57",
];

struct Anchor {
    time: DateTime<FixedOffset>,
    received: Instant,
    source: &'static str,
}

impl Anchor {
    fn at(&self, elapsed: Duration) -> DateTime<FixedOffset> {
        self.time + chrono::Duration::from_std(elapsed).expect("clock elapsed within range")
    }
}

#[derive(Default)]
struct State {
    anchor: Option<Anchor>,
    error: Option<String>,
}

static STATE: OnceLock<RwLock<State>> = OnceLock::new();

fn state() -> &'static RwLock<State> {
    STATE.get_or_init(|| RwLock::new(State::default()))
}

pub fn network_now() -> Option<DateTime<FixedOffset>> {
    let guard = state().read().expect("clock lock");
    guard.anchor.as_ref().map(|a| a.at(a.received.elapsed()))
}

pub fn now() -> DateTime<FixedOffset> {
    // Startup/offline audit timestamps only; live trading logic waits for network_now().
    network_now().unwrap_or_else(|| Utc::now().with_timezone(&BEIJING))
}

pub fn is_synced() -> bool {
    network_now().is_some()
}

pub fn status() -> String {
    let guard = state().read().expect("clock lock");
    match &guard.anchor {
        Some(a) => format!(
            "北京时间 UTC+8 · 网络校时 {}（{}）{}",
            a.time.format("%m-%d %H:%M:%S"),
            a.source,
            if guard.error.is_some() {
                "；网络暂不可用，沿用已校准时钟"
            } else {
                ""
            }
        ),
        None => format!(
            "北京时间尚未校准{}",
            guard
                .error
                .as_ref()
                .map(|e| format!("：{e}；稍后自动重试"))
                .unwrap_or_default()
        ),
    }
}

pub fn start() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::spawn(|| loop {
            let success = sync().is_ok();
            std::thread::sleep(Duration::from_secs(if success { 1800 } else { 60 }));
        });
    });
}

pub fn sync() -> anyhow::Result<()> {
    let result = fetch_anchor();
    let mut guard = state().write().expect("clock lock");
    match result {
        Ok(anchor) => {
            guard.anchor = Some(anchor);
            guard.error = None;
            Ok(())
        }
        Err(e) => {
            guard.error = Some(format!("{e:#}"));
            Err(e)
        }
    }
}

fn fetch_anchor() -> anyhow::Result<Anchor> {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 mo-stock-watch")
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut errors = Vec::new();
    for source in SOURCES {
        let started = Instant::now();
        let result = (|| -> anyhow::Result<DateTime<FixedOffset>> {
            let response = client
                .get(source)
                .query(&[(
                    "_clock",
                    format!(
                        "{}-{}",
                        std::process::id(),
                        NONCE.fetch_add(1, Ordering::Relaxed)
                    ),
                )])
                .header(CACHE_CONTROL, "no-cache, no-store")
                .send()?
                .error_for_status()?;
            server_time(response.headers(), started.elapsed())
        })();
        match result {
            Ok(time) => {
                return Ok(Anchor {
                    time,
                    received: Instant::now(),
                    source,
                })
            }
            Err(e) => errors.push(format!("{source}: {e:#}")),
        }
    }
    bail!("网络校时失败：{}", errors.join(" | "))
}

fn server_time(headers: &HeaderMap, elapsed: Duration) -> anyhow::Result<DateTime<FixedOffset>> {
    if elapsed > Duration::from_secs(5) {
        bail!("校时响应过慢");
    }
    if let Some(age) = headers.get(AGE) {
        if age.to_str()?.parse::<u64>()? > 0 {
            bail!("拒绝缓存的服务器时间");
        }
    }
    let date = headers.get(DATE).context("服务器未提供时间")?.to_str()?;
    let time = DateTime::parse_from_rfc2822(date).context("服务器时间格式无效")?;
    Ok(time.with_timezone(&BEIJING) + chrono::Duration::from_std(elapsed / 2)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn server_date_becomes_beijing_and_crosses_midnight() {
        let mut headers = HeaderMap::new();
        headers.insert(DATE, "Tue, 20 Oct 2026 16:30:00 GMT".parse().unwrap());
        let time = server_time(&headers, Duration::from_millis(200)).unwrap();
        assert_eq!(
            time.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            "2026-10-21 00:30:00 +08:00"
        );
        assert_eq!(time.timestamp_subsec_millis(), 100);
    }

    #[test]
    fn missing_invalid_cached_or_slow_time_is_rejected() {
        let mut headers = HeaderMap::new();
        assert!(server_time(&headers, Duration::ZERO).is_err());
        headers.insert(DATE, "bad".parse().unwrap());
        assert!(server_time(&headers, Duration::ZERO).is_err());
        headers.insert(DATE, "Wed, 21 Oct 2026 02:00:00 GMT".parse().unwrap());
        headers.insert(AGE, "120".parse().unwrap());
        assert!(server_time(&headers, Duration::ZERO).is_err());
        headers.remove(AGE);
        assert!(server_time(&headers, Duration::from_secs(6)).is_err());
    }

    #[test]
    fn synced_clock_advances_without_reading_system_clock() {
        let anchor = Anchor {
            time: BEIJING.with_ymd_and_hms(2026, 10, 21, 23, 59, 59).unwrap(),
            received: Instant::now(),
            source: "fixture",
        };
        assert_eq!(
            anchor
                .at(Duration::from_secs(2))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2026-10-22 00:00:01"
        );
    }
}
