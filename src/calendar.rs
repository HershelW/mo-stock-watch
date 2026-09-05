use anyhow::{bail, Context};
use chrono::{Datelike, NaiveDate, Weekday};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    sync::{OnceLock, RwLock},
    time::Duration,
};

use crate::config;

const SSE_CALENDAR_URL: &str = "https://www.sse.com.cn/disclosure/dealinstruc/closed/";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalendarFile {
    year: i32,
    source: String,
    holidays: Vec<NaiveDate>,
}

static HOLIDAYS: OnceLock<RwLock<HashSet<NaiveDate>>> = OnceLock::new();

pub fn is_trading_day(date: NaiveDate) -> bool {
    if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    !holidays().read().expect("calendar lock").contains(&date)
}

pub fn refresh_from_sse(year: i32) -> anyhow::Result<usize> {
    let html = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 mo-stock-watch/0.2")
        .timeout(Duration::from_secs(15))
        .build()?
        .get(SSE_CALENDAR_URL)
        .send()
        .context("请求上交所休市安排失败")?
        .error_for_status()
        .context("上交所休市安排返回错误状态")?
        .text()
        .context("读取上交所休市安排失败")?;
    let parsed = parse_holiday_ranges(&html, year);
    if parsed.is_empty() {
        bail!("没有从上交所页面解析到 {year} 年休市日期");
    }
    let file = CalendarFile {
        year,
        source: SSE_CALENDAR_URL.to_owned(),
        holidays: {
            let mut values = parsed.iter().copied().collect::<Vec<_>>();
            values.sort();
            values
        },
    };
    fs::create_dir_all(config::app_dir()).context("创建应用数据目录失败")?;
    fs::write(
        calendar_path(),
        serde_json::to_string_pretty(&file).context("序列化交易日历失败")?,
    )
    .context("保存交易日历失败")?;
    *holidays().write().expect("calendar lock") = merged_holidays(Some(file));
    Ok(parsed.len())
}

pub fn source_label() -> String {
    if let Ok(raw) = fs::read_to_string(calendar_path()) {
        if let Ok(file) = serde_json::from_str::<CalendarFile>(&raw) {
            return format!("上交所 {} 年日历", file.year);
        }
    }
    "内置 2026 年日历".to_owned()
}

fn holidays() -> &'static RwLock<HashSet<NaiveDate>> {
    HOLIDAYS.get_or_init(|| RwLock::new(merged_holidays(load_calendar_file())))
}

fn load_calendar_file() -> Option<CalendarFile> {
    let raw = fs::read_to_string(calendar_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn calendar_path() -> std::path::PathBuf {
    config::app_dir().join("market_holidays.json")
}

fn merged_holidays(file: Option<CalendarFile>) -> HashSet<NaiveDate> {
    let mut dates = built_in_2026();
    if let Some(file) = file {
        dates.extend(file.holidays);
    }
    dates
}

fn built_in_2026() -> HashSet<NaiveDate> {
    [
        (1, 1, 1, 3),
        (2, 15, 2, 23),
        (4, 4, 4, 6),
        (5, 1, 5, 5),
        (6, 19, 6, 21),
        (9, 25, 9, 27),
        (10, 1, 10, 7),
    ]
    .into_iter()
    .flat_map(|(start_month, start_day, end_month, end_day)| {
        date_range(2026, start_month, start_day, end_month, end_day)
    })
    .collect()
}

fn parse_holiday_ranges(html: &str, year: i32) -> HashSet<NaiveDate> {
    let marker = format!("{year}年休市安排");
    let Some(start) = html.find(&marker) else {
        return HashSet::new();
    };
    let section = &html[start..html.len().min(start + 15_000)];
    let range_re = Regex::new(
        r"(?P<sm>\d{1,2})月(?P<sd>\d{1,2})日[^。；<]{0,40}?至(?P<em>\d{1,2})月(?P<ed>\d{1,2})日",
    )
    .expect("valid calendar regex");
    let mut dates = HashSet::new();
    for capture in range_re.captures_iter(section) {
        let values = ["sm", "sd", "em", "ed"].map(|name| capture[name].parse::<u32>().ok());
        if let [Some(sm), Some(sd), Some(em), Some(ed)] = values {
            dates.extend(date_range(year, sm, sd, em, ed));
        }
    }
    dates
}

fn date_range(
    year: i32,
    start_month: u32,
    start_day: u32,
    end_month: u32,
    end_day: u32,
) -> Vec<NaiveDate> {
    let Some(mut date) = NaiveDate::from_ymd_opt(year, start_month, start_day) else {
        return Vec::new();
    };
    let Some(end) = NaiveDate::from_ymd_opt(year, end_month, end_day) else {
        return Vec::new();
    };
    let mut dates = Vec::new();
    while date <= end {
        dates.push(date);
        date += chrono::Duration::days(1);
    }
    dates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_calendar_skips_2026_dragon_boat_holiday() {
        assert!(!is_trading_day(
            NaiveDate::from_ymd_opt(2026, 6, 19).unwrap()
        ));
        assert!(is_trading_day(
            NaiveDate::from_ymd_opt(2026, 6, 22).unwrap()
        ));
    }

    #[test]
    fn parses_ranges_from_sse_text() {
        let text = "2026年休市安排 端午节：6月19日（星期五）至6月21日（星期日）休市";
        let dates = parse_holiday_ranges(text, 2026);
        assert_eq!(dates.len(), 3);
    }
}
