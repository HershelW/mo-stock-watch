use anyhow::Context;
use serde::Deserialize;
use std::{
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/HershelW/mo-stock-watch/releases/latest";

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub tag: String,
    pub name: String,
    pub url: String,
    pub newer: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
}

pub fn spawn_check() -> Receiver<anyhow::Result<UpdateInfo>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(check());
    });
    rx
}

pub fn open_release(url: &str) {
    let mut command = Command::new("cmd.exe");
    command.args(["/c", "start", "", url]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let _ = command.spawn();
}

fn check() -> anyhow::Result<UpdateInfo> {
    let release: GithubRelease = reqwest::blocking::Client::builder()
        .user_agent("mo-stock-watch-update-check")
        .timeout(Duration::from_secs(10))
        .build()?
        .get(LATEST_RELEASE_URL)
        .send()
        .context("检查 GitHub Release 失败")?
        .error_for_status()
        .context("GitHub Release 返回错误状态")?
        .json()
        .context("解析 GitHub Release 失败")?;
    let current = env!("CARGO_PKG_VERSION");
    let tag_version = release.tag_name.trim_start_matches('v');
    Ok(UpdateInfo {
        newer: version_tuple(tag_version) > version_tuple(current),
        tag: release.tag_name,
        name: release.name.unwrap_or_else(|| "新版本".to_owned()),
        url: release.html_url,
    })
}

fn version_tuple(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').filter_map(|part| part.parse().ok());
    (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    )
}
