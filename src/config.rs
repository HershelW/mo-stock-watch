use crate::{
    credential,
    portfolio::{Holding, Portfolio},
};
use anyhow::Context;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub refresh_interval_secs: u64,
    pub always_on_top: bool,
    pub opacity: f32,
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub ultra_compact: bool,
    #[serde(default)]
    pub normal_window_size: Option<[f32; 2]>,
    #[serde(default = "default_ocr_model")]
    pub ocr_model: String,
    #[serde(default = "default_analysis_model")]
    pub analysis_model: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default = "default_openai_base_url")]
    pub openai_base_url: String,
    #[serde(default = "default_model_list")]
    pub available_models: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            refresh_interval_secs: 15,
            always_on_top: true,
            opacity: 0.94,
            font_scale: default_font_scale(),
            theme: default_theme(),
            ultra_compact: false,
            normal_window_size: None,
            ocr_model: default_ocr_model(),
            analysis_model: default_analysis_model(),
            openai_api_key: String::new(),
            openai_base_url: default_openai_base_url(),
            available_models: default_model_list(),
        }
    }
}

fn default_font_scale() -> f32 {
    1.0
}

fn default_theme() -> String {
    "copper".to_owned()
}

fn default_ocr_model() -> String {
    "gpt-5.4-mini".to_owned()
}

fn default_analysis_model() -> String {
    "gpt-5.4-mini".to_owned()
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".to_owned()
}

fn default_model_list() -> Vec<String> {
    vec![
        default_ocr_model(),
        "gpt-5.4".to_owned(),
        "gpt-5.3-mini".to_owned(),
    ]
}

pub fn app_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("MO_STOCK_APP_DIR") {
        return PathBuf::from(path);
    }
    dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("mo-stock-watch")
}

pub fn portfolio_path() -> PathBuf {
    app_dir().join("portfolio.json")
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn load_portfolio() -> anyhow::Result<Portfolio> {
    load_portfolio_at(&portfolio_path())
}

fn load_portfolio_at(path: &Path) -> anyhow::Result<Portfolio> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if path.with_extension("json.previous").exists() {
                anyhow::bail!("发现中断的存档写入，请先恢复备份");
            }
            return Ok(Portfolio::default());
        }
        Err(e) => return Err(e).context("读取持仓失败；已禁止自动覆盖"),
    };
    let mut portfolio = load_portfolio_from_str(&raw)?;
    portfolio.loaded_from = Some(raw);
    Ok(portfolio)
}

pub fn save_portfolio(portfolio: &mut Portfolio) -> anyhow::Result<()> {
    write_portfolio(portfolio, true)
}

pub fn save_portfolio_quiet(portfolio: &mut Portfolio) -> anyhow::Result<()> {
    write_portfolio(portfolio, false)
}

fn write_portfolio(portfolio: &mut Portfolio, create_backup: bool) -> anyhow::Result<()> {
    portfolio.validate()?;
    let _lock = DataLock::acquire(&app_dir())?;
    let current = read_existing(&portfolio_path())?;
    if current != portfolio.loaded_from {
        anyhow::bail!("持仓已被其他进程修改，请重新加载后再保存");
    }
    portfolio.normalize();
    portfolio.last_saved_at = Some(Local::now());
    fs::create_dir_all(app_dir()).context("create app data dir")?;
    if create_backup {
        backup_current_portfolio()?;
    }
    atomic_write_json(&portfolio_path(), portfolio).context("write portfolio")?;
    portfolio.loaded_from = read_existing(&portfolio_path())?;
    Ok(())
}

pub fn load_settings() -> AppSettings {
    let path = settings_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return AppSettings::default();
    };

    let mut settings: AppSettings = serde_json::from_str(&raw).unwrap_or_default();
    if settings.openai_api_key.trim().is_empty() {
        settings.openai_api_key = credential::load_api_key().unwrap_or_default();
    } else if credential::save_api_key(&settings.openai_api_key).is_ok() {
        let _ = write_sanitized_settings(&settings);
    }
    settings
}

pub fn save_settings(settings: &AppSettings) -> anyhow::Result<()> {
    fs::create_dir_all(app_dir()).context("create app data dir")?;
    if !settings.openai_api_key.trim().is_empty() {
        credential::save_api_key(&settings.openai_api_key)?;
    }
    write_sanitized_settings(settings)
}

pub fn list_portfolio_backups() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(backup_dir()) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    paths
}

pub fn restore_latest_portfolio_backup() -> anyhow::Result<Portfolio> {
    let latest = list_portfolio_backups()
        .into_iter()
        .next()
        .context("没有可恢复的自动备份")?;
    restore_portfolio_backup(&latest)
}

pub fn restore_portfolio_backup(path: &Path) -> anyhow::Result<Portfolio> {
    let backup_root = backup_dir().canonicalize().unwrap_or_else(|_| backup_dir());
    let resolved = path.canonicalize().context("备份文件不存在")?;
    if !resolved.starts_with(&backup_root) {
        anyhow::bail!("拒绝恢复应用目录之外的文件");
    }
    let raw = fs::read_to_string(&resolved).context("读取备份失败")?;
    let mut portfolio = load_portfolio_from_str(&raw).context("备份格式无效")?;
    backup_current_portfolio()?;
    portfolio.loaded_from = read_existing(&portfolio_path())?;
    write_portfolio(&mut portfolio, false)?;
    Ok(portfolio)
}

pub fn backup_dir() -> PathBuf {
    app_dir().join("backups")
}

fn backup_current_portfolio() -> anyhow::Result<()> {
    let source = portfolio_path();
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(backup_dir()).context("create backup dir")?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f");
    let destination = backup_dir().join(format!("portfolio-{stamp}.json"));
    fs::copy(&source, destination).context("backup portfolio")?;
    trim_backups(20)
}

fn trim_backups(keep: usize) -> anyhow::Result<()> {
    let paths = list_portfolio_backups();
    for path in paths.into_iter().skip(keep) {
        fs::remove_file(path).context("remove old portfolio backup")?;
    }
    Ok(())
}

fn write_sanitized_settings(settings: &AppSettings) -> anyhow::Result<()> {
    let mut public_settings = settings.clone();
    public_settings.openai_api_key.clear();
    atomic_write_json(&settings_path(), &public_settings).context("write settings")
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(value).context("serialize json")?;
    let temp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp).context("create temporary json")?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        let previous = path.with_extension("json.previous");
        let _ = fs::remove_file(&previous);
        fs::rename(path, &previous).context("move previous json")?;
        if let Err(error) = fs::rename(&temp, path) {
            let _ = fs::rename(&previous, path);
            return Err(error).context("activate new json");
        }
        // Retain the previous valid revision for interrupted-write recovery.
    } else {
        fs::rename(&temp, path).context("activate new json")?;
    }
    Ok(())
}

pub fn load_portfolio_from_str(raw: &str) -> anyhow::Result<Portfolio> {
    let value: serde_json::Value = serde_json::from_str(raw).context("parse portfolio json")?;
    if value.get("accounts").is_some() {
        let mut portfolio: Portfolio =
            serde_json::from_str(raw).context("parse account portfolio")?;
        portfolio.validate()?;
        portfolio.normalize();
        portfolio.validate()?;
        return Ok(portfolio);
    }

    let legacy: LegacyPortfolio = serde_json::from_str(raw).context("parse legacy portfolio")?;
    let portfolio = Portfolio::from_legacy_holdings(legacy.holdings, legacy.last_saved_at);
    portfolio.validate()?;
    Ok(portfolio)
}

fn read_existing(path: &Path) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

struct DataLock(PathBuf);
impl DataLock {
    fn acquire(root: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(root)?;
        let path = root.join("portfolio.lock");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("持仓更新正在进行；若进程异常退出，请确认后清理 portfolio.lock")?;
        Ok(Self(path))
    }
}
impl Drop for DataLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug, Deserialize)]
struct LegacyPortfolio {
    holdings: Vec<Holding>,
    #[serde(default)]
    last_saved_at: Option<chrono::DateTime<Local>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_state_never_becomes_a_sample_or_changes_disk() {
        let root = std::env::temp_dir().join(format!("mo-stock-invalid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("portfolio.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(load_portfolio_at(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
        fs::remove_file(&path).unwrap();
        let p = load_portfolio_at(&path).unwrap();
        assert!(p.holdings().next().is_none());
        fs::write(path.with_extension("json.previous"), "{}").unwrap();
        assert!(load_portfolio_at(&path).is_err());
        fs::remove_file(path.with_extension("json.previous")).unwrap();
        fs::remove_dir(root).unwrap();
        let mut value = serde_json::to_value(Portfolio::default()).unwrap();
        value["accounts"] = serde_json::json!([]);
        assert!(load_portfolio_from_str(&value.to_string()).is_err());
        assert!(load_portfolio_from_str(r#"{"accounts":[],"accounts":[]}"#).is_err());
    }

    #[test]
    fn update_lock_excludes_other_writers() {
        let root = std::env::temp_dir().join(format!("mo-stock-lock-{}", std::process::id()));
        let first = DataLock::acquire(&root).unwrap();
        assert!(DataLock::acquire(&root).is_err());
        drop(first);
        drop(DataLock::acquire(&root).unwrap());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn old_settings_default_to_copper_theme() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"refresh_interval_secs":15,"always_on_top":true,"opacity":0.94}"#,
        )
        .expect("legacy settings should still load");

        assert_eq!(settings.theme, "copper");
    }
}
