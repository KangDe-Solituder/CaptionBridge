//! OCR credentials are borrowed from an official DeepSeek profile, never copied to settings.
use crate::{
    models::{AppSettings, OcrSettings},
    ocr::Block,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;

pub fn official_url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "请输入有效的 DeepSeek 官方 HTTPS 地址")?;
    if url.scheme() != "https"
        || url.host_str() != Some("api.deepseek.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("复用 DeepSeek 密钥时，仅允许 api.deepseek.com 的 HTTPS 地址，不支持自定义端口、查询参数或重定向".into());
    }
    Ok(url)
}

pub fn validate_config(config: &OcrSettings, settings: &AppSettings) -> Result<(), String> {
    official_url(&config.cloud_endpoint)?;
    let profile = settings
        .llm_profiles
        .iter()
        .find(|p| Some(&p.id) == config.cloud_profile_id.as_ref())
        .ok_or("请在 OCR 编辑中选择已有的 DeepSeek 官方翻译配置")?;
    official_url(&profile.config.endpoint)?;
    if !profile.api_key_configured {
        return Err("关联的 DeepSeek 配置尚未保存密钥，请先在翻译与应用中编辑".into());
    }
    Ok(())
}

pub async fn recognize(
    config: &OcrSettings,
    settings: &AppSettings,
    image: &str,
) -> Result<Vec<Block>, String> {
    validate_config(config, settings)?;
    let key = crate::secrets::profile_key(config.cloud_profile_id.as_deref().unwrap())?
        .filter(|k| !k.is_empty())
        .ok_or("关联配置的密钥不可用，请重新保存 API Key")?;
    let bytes = STANDARD.decode(image).map_err(|_| "截图编码无效")?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(190))
        .build()
        .map_err(|_| "无法建立 OCR 连接")?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name("selection.png")
        .mime_str("image/png")
        .map_err(|_| "无法准备截图")?;
    let response = client
        .post(official_url(&config.cloud_endpoint)?)
        .bearer_auth(key)
        .multipart(
            reqwest::multipart::Form::new()
                .text("purpose", "text_recognition")
                .part("file", part),
        )
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "云端 OCR 请求超时，请稍后重试"
            } else {
                "无法连接 DeepSeek OCR，请检查网络和接口地址"
            }
        })?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => "DeepSeek OCR 认证失败，请检查关联配置的密钥".into(),
            402 => "DeepSeek 账户余额不足".into(),
            429 => "DeepSeek OCR 请求过多，请稍后重试".into(),
            code => format!("DeepSeek OCR 请求失败（HTTP {code}），请检查接口地址或稍后重试"),
        });
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "接收 OCR 结果失败，请稍后重试")?;
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err("OCR 返回数据过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| "OCR 返回了无效 JSON")?;
    parse_response(&value)
}

fn clean_text(text: &str) -> String {
    let mut text = text.replace("\r\n", "\n");
    // Strip metadata, never turn its coordinates or type labels into translations.
    for tag in ["ref", "box"] {
        let start = format!("<｜{tag}｜>");
        let end = format!("<｜/{tag}｜>");
        while let Some(a) = text.find(&start) {
            if let Some(b) = text[a + start.len()..].find(&end) {
                text.replace_range(a..a + start.len() + b + end.len(), "");
            } else {
                text.truncate(a);
                break;
            }
        }
    }
    text.lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn parse_response(value: &Value) -> Result<Vec<Block>, String> {
    if value.get("error").is_some() {
        return Err("DeepSeek OCR 识别失败，请稍后重试".into());
    }
    if value["object"] != "ocr.result" {
        return Err("OCR 返回格式不正确，请检查接口地址".into());
    }
    let pages = value["pages"].as_array().ok_or("OCR 结果缺少页面")?;
    let mut blocks = Vec::new();
    for page in pages {
        if page["finish_reason"] != "stop" {
            return Err("OCR 结果不完整，请缩小选区后重试".into());
        }
        let text = clean_text(page["text"].as_str().ok_or("OCR 结果缺少文本")?);
        // Text recognition supplies paragraphs, not trustworthy bounding boxes/confidence.
        for paragraph in text.split("\n\n").flat_map(|p| p.split('\n')) {
            if !paragraph.chars().any(|c| {
                !c.is_whitespace()
                    && !matches!(c, '#' | '*' | '_' | '`' | '-' | '|' | '~' | '>' | '<')
            }) {
                continue;
            }
            let chars: Vec<char> = paragraph.chars().collect();
            for chunk in chars.chunks(2400) {
                blocks.push(Block {
                    id: blocks.len() as u32 + 1,
                    text: chunk.iter().collect(),
                    confidence: None,
                    ..Default::default()
                });
                if blocks.len() > 100 {
                    return Err("文字块超过 100 个，请缩小截图范围".into());
                }
            }
        }
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn credentials_cannot_leave_the_official_origin() {
        assert!(official_url("https://api.deepseek.com/v1/tools/ocr").is_ok());
        for url in [
            "http://api.deepseek.com/v1",
            "https://api.deepseek.com.evil.test/v1",
            "https://api.deepseek.com@evil.test/",
            "https://api.deepseek.com:444/v1",
            "https://api.deepseek.com/v1?key=x",
        ] {
            assert!(official_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn response_handles_empty_metadata_errors_and_truncation() {
        let response = |text: &str| json!({"object":"ocr.result","pages":[{"text":text,"finish_reason":"stop"}]});
        assert!(parse_response(&response(
            " \n<｜ref｜>title<｜/ref｜><｜box｜>[[1,2,3,4]]<｜/box｜>\n**"
        ))
        .unwrap()
        .is_empty());
        let blocks = parse_response(&response("こんにちは🌸\n\n* **Title:** hello")).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "こんにちは🌸");
        assert_eq!(blocks[0].confidence, None);
        assert_eq!(parse_response(&response("🌸")).unwrap()[0].text, "🌸");
        assert!(parse_response(&json!({"error":{"message":"failure"}})).is_err());
        assert!(parse_response(
            &json!({"object":"ocr.result","pages":[{"text":"partial","finish_reason":"length"}]})
        )
        .is_err());
    }
}
