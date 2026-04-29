use crate::portfolio::Portfolio;
use anyhow::Context;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub refresh_interval_secs: u64,
    pub always_on_top: bool,
    pub opacity: f32,
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
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

pub fn load_portfolio() -> Portfolio {
    let path = portfolio_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Portfolio::default();
    };

    serde_json::from_str(&raw).unwrap_or_else(|_| Portfolio::default())
}

pub fn save_portfolio(portfolio: &mut Portfolio) -> anyhow::Result<()> {
    portfolio.normalize();
    portfolio.last_saved_at = Some(Local::now());
    fs::create_dir_all(app_dir()).context("create app data dir")?;
    fs::write(
        portfolio_path(),
        serde_json::to_string_pretty(portfolio).context("serialize portfolio")?,
    )
    .context("write portfolio")
}

pub fn load_settings() -> AppSettings {
    let path = settings_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return AppSettings::default();
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_settings(settings: &AppSettings) -> anyhow::Result<()> {
    fs::create_dir_all(app_dir()).context("create app data dir")?;
    fs::write(
        settings_path(),
        serde_json::to_string_pretty(settings).context("serialize settings")?,
    )
    .context("write settings")
}
