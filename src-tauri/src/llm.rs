use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use tokio::time::{timeout, timeout_at, Instant as TokioInstant};

use crate::models::{AppSettings, TranslationMode, TranslationRequest, TranslationResult};

pub fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

const LIVE_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(2);

fn first_token_timeout(settings: &AppSettings, mode: &TranslationMode) -> Option<Duration> {
    matches!(mode, TranslationMode::LiveCaption)
        .then(|| Duration::from_millis(settings.llm.timeout_milliseconds.clamp(500, 5_000)))
}

fn first_token_timeout_error(limit: Duration, elapsed: Duration) -> String {
    format!(
        "实时字幕等待首 token 超过 {:.1} 秒（实际 {}ms），已取消本次请求",
        limit.as_secs_f64(),
        elapsed.as_millis()
    )
}

pub fn create_payload(
    settings: &AppSettings,
    request: &TranslationRequest,
) -> Result<Value, String> {
    let (task, style) = match request.mode {
        TranslationMode::Selection => ("selection translation", "natural and concise"),
        TranslationMode::LiveCaption => ("live caption translation", "short, stable, and readable"),
    };

    let max_tokens = match request.mode {
        TranslationMode::LiveCaption => settings.llm.max_tokens.min(256),
        TranslationMode::Selection => settings.llm.max_tokens.min(768),
    };
    let context_hint = if request.context.is_empty() {
        String::new()
    } else {
        let context = request
            .context
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(" Previous caption context (use only for terminology and continuity):\n{context}")
    };
    let mut payload = json!({
        "model": settings.llm.model,
        "stream": true,
        "stream_options": { "include_usage": false },
        "temperature": settings.llm.temperature,
        "max_tokens": max_tokens,
        "messages": [
            {
                "role": "system",
                "content": format!("You are doing {task}. Translate into {}. Keep the result {style}. Return only the translation.{context_hint}", request.target_language)
            },
            {
                "role": "user",
                "content": request.source_text
            }
        ]
    });

    let thinking_enabled =
        settings.llm.thinking_enabled && matches!(request.mode, TranslationMode::Selection);
    let deepseek_v4 = settings.llm.endpoint.contains("api.deepseek.com")
        || settings.llm.model.starts_with("deepseek-v4");
    if deepseek_v4 {
        payload["thinking"] = json!({
            "type": if thinking_enabled { "enabled" } else { "disabled" }
        });
    } else if thinking_enabled {
        payload["enable_thinking"] = Value::Bool(true);
    }

    let extra = settings.llm.extra_body_json.trim();
    if !extra.is_empty() {
        let extra_value: Value = serde_json::from_str(extra).map_err(|error| error.to_string())?;
        if !extra_value.is_object() {
            return Err("Extra Body JSON 必须是对象".to_string());
        }
        deep_merge(&mut payload, extra_value);
    }

    Ok(payload)
}

pub async fn translate(
    settings: AppSettings,
    api_key: String,
    request: TranslationRequest,
) -> Result<TranslationResult, String> {
    translate_with_delta(settings, api_key, request, |_| {}).await
}

pub async fn translate_with_delta<F>(
    settings: AppSettings,
    api_key: String,
    request: TranslationRequest,
    mut on_delta: F,
) -> Result<TranslationResult, String>
where
    F: FnMut(String) + Send,
{
    let started = Instant::now();
    let payload = create_payload(&settings, &request)?;
    let endpoint = normalize_endpoint(&settings.llm.endpoint);
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // Translation providers are expected to be reachable directly in China.
            // Do not inherit HTTP(S)_PROXY or the Windows user proxy from the host.
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("valid shared HTTP client")
    });
    let first_token_timeout = first_token_timeout(&settings, &request.mode);
    let first_token_deadline = first_token_timeout.map(|limit| TokioInstant::now() + limit);
    let is_live_caption = matches!(request.mode, TranslationMode::LiveCaption);

    let translation = async {
        let send = client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {api_key}"))
            .json(&payload)
            .send();
        let mut response =
            if let (Some(limit), Some(deadline)) = (first_token_timeout, first_token_deadline) {
                timeout_at(deadline, send)
                    .await
                    .map_err(|_| first_token_timeout_error(limit, started.elapsed()))?
                    .map_err(|error| error.to_string())?
            } else {
                send.await.map_err(|error| error.to_string())?
            };
        let status = response.status();
        if !status.is_success() {
            let read_error_body = response.json();
            let body: Value = if let (Some(limit), Some(deadline)) =
                (first_token_timeout, first_token_deadline)
            {
                timeout_at(deadline, read_error_body)
                    .await
                    .map_err(|_| first_token_timeout_error(limit, started.elapsed()))?
                    .map_err(|error| error.to_string())?
            } else {
                read_error_body.await.map_err(|error| error.to_string())?
            };
            let message = body
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("LLM 请求失败");
            return Err(format!("{status}: {message}"));
        }

        let mut pending = Vec::<u8>::new();
        let mut translated = String::new();
        let mut first_token_ms = 0;
        let mut stream_warning = None;
        let mut stream_done = false;
        loop {
            let next_chunk = if translated.is_empty() {
                if let (Some(limit), Some(deadline)) = (first_token_timeout, first_token_deadline) {
                    timeout_at(deadline, response.chunk())
                        .await
                        .map_err(|_| first_token_timeout_error(limit, started.elapsed()))?
                        .map_err(|error| error.to_string())?
                } else {
                    response.chunk().await.map_err(|error| error.to_string())?
                }
            } else if is_live_caption {
                match timeout(LIVE_STREAM_IDLE_TIMEOUT, response.chunk()).await {
                    Ok(Ok(chunk)) => chunk,
                    Ok(Err(error)) => {
                        stream_warning = Some(format!(
                            "实时字幕流读取中断，已保留部分译文（首 token {}ms，总耗时 {}ms）：{error}",
                            first_token_ms,
                            started.elapsed().as_millis()
                        ));
                        break;
                    }
                    Err(_) => {
                        stream_warning = Some(format!(
                            "实时字幕流连续 {:.1} 秒没有新数据，已保留部分译文（首 token {}ms，总耗时 {}ms）",
                            LIVE_STREAM_IDLE_TIMEOUT.as_secs_f64(),
                            first_token_ms,
                            started.elapsed().as_millis()
                        ));
                        break;
                    }
                }
            } else {
                response.chunk().await.map_err(|error| error.to_string())?
            };
            let Some(chunk) = next_chunk else {
                break;
            };
            pending.extend_from_slice(&chunk);
            while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line = pending.drain(..=newline).collect::<Vec<_>>();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                if data == "[DONE]" {
                    stream_done = true;
                    break;
                }
                let Ok(value) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                if let Some(delta) = value
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    if first_token_ms == 0 {
                        first_token_ms = started.elapsed().as_millis();
                    }
                    translated.push_str(delta);
                    on_delta(delta.to_string());
                }
            }
            if stream_done {
                break;
            }
        }
        if translated.trim().is_empty() {
            return Err("LLM 流式响应没有返回翻译内容".to_string());
        }
        Ok::<_, String>((
            translated.trim().to_string(),
            first_token_ms,
            stream_warning,
        ))
    };
    let (translated, first_token_ms, stream_warning) = translation.await?;

    Ok(TranslationResult {
        source_text: request.source_text,
        translated_text: translated,
        model: settings.llm.model,
        latency_ms: started.elapsed().as_millis(),
        first_token_ms,
        cached: false,
        error: stream_warning,
    })
}

#[cfg(test)]
fn extract_content(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                deep_merge(target.entry(key).or_insert(Value::Null), value);
            }
        }
        (target, source) => *target = source,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_chat_completions_endpoint() {
        assert_eq!(
            normalize_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            normalize_endpoint("https://example.com/v1/chat/completions"),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn only_realtime_translation_has_a_first_token_timeout() {
        let mut settings = AppSettings::default();
        settings.llm.timeout_milliseconds = 9_000;
        assert_eq!(
            first_token_timeout(&settings, &TranslationMode::LiveCaption),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            first_token_timeout(&settings, &TranslationMode::Selection),
            None
        );
    }

    #[test]
    fn first_token_timeout_message_uses_the_configured_limit_and_elapsed_time() {
        assert_eq!(
            first_token_timeout_error(Duration::from_millis(3_250), Duration::from_millis(3_257)),
            "实时字幕等待首 token 超过 3.2 秒（实际 3257ms），已取消本次请求"
        );
    }

    #[test]
    fn merges_extra_body() {
        let mut settings = AppSettings::default();
        settings.llm.extra_body_json =
            r#"{"thinking":{"type":"disabled"},"temperature":0.1}"#.to_string();
        let request = TranslationRequest {
            mode: TranslationMode::Selection,
            source_text: "Hello".to_string(),
            context: Vec::new(),
            source_language: "auto".to_string(),
            target_language: "中文".to_string(),
        };

        let payload = create_payload(&settings, &request).unwrap();
        assert_eq!(payload["thinking"], json!({"type":"disabled"}));
        assert_eq!(payload["temperature"], json!(0.1));
    }

    #[test]
    fn adds_thinking_only_when_enabled_and_allows_override() {
        let mut settings = AppSettings::default();
        settings.llm.thinking_enabled = true;
        let request = TranslationRequest {
            mode: TranslationMode::Selection,
            source_text: "Hello".to_string(),
            context: Vec::new(),
            source_language: "auto".to_string(),
            target_language: "中文".to_string(),
        };
        assert_eq!(
            create_payload(&settings, &request).unwrap()["enable_thinking"],
            json!(true)
        );

        settings.llm.extra_body_json = r#"{"enable_thinking":false}"#.to_string();
        assert_eq!(
            create_payload(&settings, &request).unwrap()["enable_thinking"],
            json!(false)
        );
    }

    #[test]
    fn explicitly_disables_deepseek_v4_thinking_for_fast_translation() {
        let mut settings = AppSettings::default();
        settings.llm.endpoint = "https://api.deepseek.com/v1".to_string();
        settings.llm.model = "deepseek-v4-flash".to_string();
        settings.llm.thinking_enabled = false;
        let request = TranslationRequest {
            mode: TranslationMode::Selection,
            source_text: "Hello".to_string(),
            context: Vec::new(),
            source_language: "auto".to_string(),
            target_language: "中文".to_string(),
        };

        let payload = create_payload(&settings, &request).unwrap();
        assert_eq!(payload["thinking"], json!({"type": "disabled"}));
        assert_eq!(payload["stream"], json!(true));
    }

    #[test]
    fn includes_live_caption_context_without_changing_user_text() {
        let settings = AppSettings::default();
        let request = TranslationRequest {
            mode: TranslationMode::LiveCaption,
            source_text: "Current sentence".to_string(),
            source_language: "auto".to_string(),
            target_language: "Chinese".to_string(),
            context: vec!["Previous sentence".to_string()],
        };
        let payload = create_payload(&settings, &request).unwrap();
        let system = payload["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("Previous sentence"));
        assert_eq!(payload["messages"][1]["content"], "Current sentence");
    }

    #[test]
    fn extracts_content() {
        let value = json!({"choices":[{"message":{"content":" 你好 "}}]});
        assert_eq!(extract_content(&value), Some("你好".to_string()));
    }
}
