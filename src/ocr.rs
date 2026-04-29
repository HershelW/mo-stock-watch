use crate::portfolio::{Holding, Market};
use anyhow::{bail, Context};
use regex::Regex;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn recognize_holdings_from_image(path: &Path) -> anyhow::Result<Vec<Holding>> {
    let output = Command::new(tesseract_path())
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .arg("--psm")
        .arg("6")
        .output()
        .context("启动 tesseract 失败，请确认 Tesseract OCR 已安装")?;

    if !output.status.success() {
        bail!(
            "tesseract 识别失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_holdings_from_ocr_text(&text))
}

fn tesseract_path() -> PathBuf {
    let common_paths = [
        r"C:\Program Files\Tesseract-OCR\tesseract.exe",
        r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe",
    ];

    common_paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("tesseract"))
}

pub fn parse_holdings_from_ocr_text(text: &str) -> Vec<Holding> {
    let code_re = Regex::new(r"(?P<code>\d{6})").expect("valid code regex");
    let number_re = Regex::new(r"-?\d+(?:\.\d+)?").expect("valid number regex");
    let mut holdings = Vec::new();

    for line in text.lines() {
        let Some(code_match) = code_re.captures(line) else {
            continue;
        };
        let code = code_match["code"].to_owned();
        let numbers = number_re
            .find_iter(line)
            .filter_map(|m| m.as_str().parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .collect::<Vec<_>>();

        if numbers.len() < 3 {
            continue;
        }

        let quantity = numbers
            .iter()
            .copied()
            .filter(|n| *n >= 1.0 && n.fract().abs() < f64::EPSILON)
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(numbers[1]);

        let cost_price = numbers
            .iter()
            .copied()
            .filter(|n| *n > 0.0 && *n < 10_000.0 && (*n - quantity).abs() > f64::EPSILON)
            .next()
            .unwrap_or(numbers[1]);

        let name = line
            .replace(&code, "")
            .split_whitespace()
            .find(|token| token.chars().any(|c| c as u32 > 127))
            .unwrap_or("")
            .trim()
            .to_owned();

        holdings.push(Holding {
            code: code.clone(),
            name: if name.is_empty() { code.clone() } else { name },
            quantity,
            cost_price,
            market: Market::infer(&code),
        });
    }

    holdings
}
