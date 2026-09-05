use anyhow::{bail, Context};
use std::{fs, process::Command};

use crate::config;

const CREDENTIAL_FILE: &str = "credentials.dat";

pub fn load_api_key() -> anyhow::Result<String> {
    let path = config::app_dir().join(CREDENTIAL_FILE);
    let blob = fs::read_to_string(&path).context("读取加密凭据失败")?;
    if blob.trim().is_empty() {
        return Ok(String::new());
    }

    let script = r#"
Add-Type -AssemblyName System.Security
$protected = [Convert]::FromBase64String($env:MO_STOCK_SECRET_BLOB)
$plain = [Security.Cryptography.ProtectedData]::Unprotect($protected, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
[Text.Encoding]::UTF8.GetString($plain)
"#;
    let output = powershell(script)
        .env("MO_STOCK_SECRET_BLOB", blob.trim())
        .output()
        .context("启动 Windows 凭据解密失败")?;
    if !output.status.success() {
        bail!(
            "Windows 凭据解密失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn save_api_key(api_key: &str) -> anyhow::Result<()> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Ok(());
    }

    let script = r#"
Add-Type -AssemblyName System.Security
$plain = [Text.Encoding]::UTF8.GetBytes($env:MO_STOCK_SECRET)
$protected = [Security.Cryptography.ProtectedData]::Protect($plain, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
[Convert]::ToBase64String($protected)
"#;
    let output = powershell(script)
        .env("MO_STOCK_SECRET", api_key)
        .output()
        .context("启动 Windows 凭据加密失败")?;
    if !output.status.success() {
        bail!(
            "Windows 凭据加密失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let blob = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if blob.is_empty() {
        bail!("Windows 凭据加密返回空数据");
    }
    fs::create_dir_all(config::app_dir()).context("创建应用数据目录失败")?;
    fs::write(config::app_dir().join(CREDENTIAL_FILE), blob).context("保存加密凭据失败")?;
    Ok(())
}

fn powershell(script: &str) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}
