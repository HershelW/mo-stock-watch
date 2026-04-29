use crate::portfolio::{Holding, Market};
use anyhow::{bail, Context};
use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};

pub fn recognize_holdings_with_openai(
    api_key: &str,
    base_url: &str,
    model: &str,
    image_path: &Path,
) -> anyhow::Result<Vec<Holding>> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("请先填写 OpenAI API Key");
    }
    ensure_not_codex_model(model)?;

    let bytes = fs::read(image_path).context("读取截图失败")?;
    let image_url = format!(
        "data:{};base64,{}",
        image_mime(image_path),
        general_purpose::STANDARD.encode(bytes)
    );

    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "holdings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "code": {"type": "string", "description": "6 digit A-share stock code"},
                        "name": {"type": "string"},
                        "quantity": {"type": "number"},
                        "cost_price": {"type": "number"}
                    },
                    "required": ["code", "name", "quantity", "cost_price"]
                }
            }
        },
        "required": ["holdings"]
    });

    let body = json!({
        "model": model.trim(),
        "input": [
            {
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": "You extract A-share portfolio rows from Chinese brokerage screenshots. Return only confirmed holdings. Ignore totals, available cash, watchlists, and market quotes. If unsure about a row, omit it."
                }]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "Extract stock code, stock name, holding quantity, and cost price from this Tonghuashun or Eastmoney portfolio screenshot. Return JSON matching the schema."
                    },
                    {
                        "type": "input_image",
                        "image_url": image_url
                    }
                ]
            }
        ],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "portfolio_holdings",
                "strict": true,
                "schema": schema
            }
        }
    });

    let client = reqwest::blocking::Client::builder()
        .user_agent("mo-stock-watch/0.1")
        .timeout(Duration::from_secs(60))
        .build()?;

    let response = client
        .post(format!("{}/responses", normalize_base_url(base_url)))
        .bearer_auth(api_key)
        .json(&body)
        .send();

    let response: Value = match response {
        Ok(resp) if resp.status().is_success() => resp.json().context("解析 OpenAI 响应失败")?,
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            recognize_with_chat_completions(&client, api_key, base_url, model, &image_url)
                .with_context(|| format!("Responses 失败 {status}: {text}"))?
        }
        Err(err) => recognize_with_chat_completions(&client, api_key, base_url, model, &image_url)
            .with_context(|| format!("Responses 请求失败：{err}"))?,
    };

    let text = response_text(&response).context("OpenAI 响应里没有文本结果")?;
    let parsed: Value = serde_json::from_str(&text).context("OpenAI 没有返回合法 JSON")?;
    let rows = parsed
        .get("holdings")
        .and_then(Value::as_array)
        .context("OpenAI JSON 缺少 holdings 数组")?;

    let holdings = rows
        .iter()
        .filter_map(|row| {
            let code = row
                .get("code")?
                .as_str()?
                .chars()
                .filter(|c| c.is_ascii_digit())
                .take(6)
                .collect::<String>();
            if code.len() != 6 {
                return None;
            }
            Some(Holding {
                name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&code)
                    .trim()
                    .to_owned(),
                quantity: row
                    .get("quantity")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                cost_price: row
                    .get("cost_price")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                market: Market::infer(&code),
                code,
            })
        })
        .filter(|h| h.quantity > 0.0 && h.cost_price > 0.0)
        .collect();

    Ok(holdings)
}

pub fn fetch_models(api_key: &str, base_url: &str) -> anyhow::Result<Vec<String>> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("请先填写 API Key");
    }

    let response: Value = reqwest::blocking::Client::builder()
        .user_agent("mo-stock-watch/0.1")
        .timeout(Duration::from_secs(30))
        .build()?
        .get(format!("{}/models", normalize_base_url(base_url)))
        .bearer_auth(api_key)
        .send()
        .context("请求模型列表失败")?
        .error_for_status()
        .context("模型列表接口返回错误状态")?
        .json()
        .context("解析模型列表失败")?;

    let mut models = response
        .get("data")
        .and_then(Value::as_array)
        .context("模型列表响应缺少 data 数组")?
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    models.sort();
    models.dedup();
    if models.is_empty() {
        bail!("模型列表为空");
    }

    Ok(models)
}

pub fn test_model(api_key: &str, base_url: &str, model: &str) -> anyhow::Result<String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("请先填写 API Key");
    }
    if model.trim().is_empty() {
        bail!("请先选择模型");
    }
    ensure_not_codex_model(model)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("mo-stock-watch/0.1")
        .timeout(Duration::from_secs(30))
        .build()?;

    let responses_body = json!({
        "model": model.trim(),
        "input": "Reply with exactly: ok"
    });
    let responses = client
        .post(format!("{}/responses", normalize_base_url(base_url)))
        .bearer_auth(api_key)
        .json(&responses_body)
        .send();

    let responses_error = match responses {
        Ok(resp) if resp.status().is_success() => {
            let value: Value = resp.json().context("解析 Responses 测试响应失败")?;
            let text = response_text(&value).unwrap_or_default();
            return Ok(format!("Responses OK: {}", compact_test_text(&text)));
        }
        Ok(resp) => {
            let status = resp.status();
            let text = compact_test_text(&resp.text().unwrap_or_default());
            format!("Responses 失败 {status}: {text}")
        }
        Err(err) => {
            format!("Responses 请求失败：{err}")
        }
    };

    let chat_body = json!({
        "model": model.trim(),
        "messages": [{"role": "user", "content": "Reply with exactly: ok"}]
    });
    let resp = client
        .post(format!("{}/chat/completions", normalize_base_url(base_url)))
        .bearer_auth(api_key)
        .json(&chat_body)
        .send()
        .with_context(|| format!("{responses_error}; Chat Completions 测试请求失败"))?;

    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "{}; Chat Completions 失败 {status}: {}",
            responses_error,
            compact_test_text(&text)
        );
    }
    let value: Value = serde_json::from_str(&text).context("解析 Chat Completions 测试响应失败")?;
    let output = response_text(&value).unwrap_or_default();
    Ok(format!(
        "Chat Completions OK: {}",
        compact_test_text(&output)
    ))
}

fn ensure_not_codex_model(model: &str) -> anyhow::Result<()> {
    if model.to_ascii_lowercase().contains("codex") {
        bail!("当前选择的是 Codex 模型/通道，不适合截图 OCR。请选择通用视觉模型，例如 gpt-4o、gpt-4.1、gpt-5.x-mini、gemini/claude 的 vision 模型，或使用模型列表里的非 codex 项。");
    }
    Ok(())
}

fn compact_test_text(text: &str) -> String {
    let trimmed = text.trim().replace(['\r', '\n'], " ");
    if trimmed.chars().count() > 160 {
        format!("{}...", trimmed.chars().take(160).collect::<String>())
    } else {
        trimmed
    }
}

fn recognize_with_chat_completions(
    client: &reqwest::blocking::Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    image_url: &str,
) -> anyhow::Result<Value> {
    let body = json!({
        "model": model.trim(),
        "messages": [
            {
                "role": "system",
                "content": "You extract A-share portfolio rows from Chinese brokerage screenshots. Return only JSON: {\"holdings\":[{\"code\":\"600519\",\"name\":\"贵州茅台\",\"quantity\":100,\"cost_price\":1500.0}]}. Ignore totals, available cash, watchlists, and market quotes."
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "Extract stock code, stock name, holding quantity, and cost price from this Tonghuashun or Eastmoney portfolio screenshot."
                    },
                    {
                        "type": "image_url",
                        "image_url": {"url": image_url}
                    }
                ]
            }
        ],
        "response_format": {"type": "json_object"}
    });

    let response: Value = client
        .post(format!("{}/chat/completions", normalize_base_url(base_url)))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("请求 Chat Completions 失败")?
        .error_for_status()
        .context("Chat Completions 返回错误状态")?
        .json()
        .context("解析 Chat Completions 响应失败")?;

    Ok(response)
}

fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        "https://api.openai.com/v1".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn response_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_owned());
    }

    if let Some(text) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        return Some(text.to_owned());
    }

    value
        .get("output")?
        .as_array()?
        .iter()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| content.get("output_text").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
}

fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}
