use crate::{
    asr_worker::{AsrWorker, WorkerEvent},
    models::{AppSettings, CloudAsrConfig, CloudAsrProvider},
    AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::mpsc;

pub fn official_url(value: &str, provider: CloudAsrProvider) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "请输入官方 ASR HTTPS 地址")?;
    if url.scheme() != "https"
        || url.host_str() != Some(provider.host())
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "复用密钥时仅支持 {} 的官方 HTTPS 地址",
            provider.host()
        ));
    }
    Ok(url)
}

pub fn validate(
    config: &CloudAsrConfig,
    provider: CloudAsrProvider,
    settings: &AppSettings,
) -> Result<(), String> {
    official_url(&config.endpoint, provider)?;
    let profile = settings
        .llm_profiles
        .iter()
        .find(|p| Some(&p.id) == config.profile_id.as_ref())
        .ok_or("请先关联已保存密钥的同服务商翻译配置")?;
    official_url(&profile.config.endpoint, provider)?;
    if !profile.api_key_configured {
        return Err("关联配置尚未保存密钥".into());
    }
    if config.proxy_enabled {
        if config.proxy_url.trim().is_empty() {
            return Err("启用云端 ASR 代理后请填写地址".into());
        }
        crate::llm::validate_proxy(&config.proxy_url)?;
    }
    Ok(())
}

pub struct Client {
    http: reqwest::Client,
    key: String,
    url: reqwest::Url,
    provider: CloudAsrProvider,
}
impl Client {
    pub fn new(
        config: &CloudAsrConfig,
        provider: CloudAsrProvider,
        settings: &AppSettings,
    ) -> Result<Self, String> {
        validate(config, provider, settings)?;
        let key = crate::secrets::profile_key(config.profile_id.as_deref().unwrap())?
            .filter(|k| !k.is_empty())
            .ok_or("关联的 ASR 密钥不可用，请重新保存")?;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15));
        if config.proxy_enabled {
            builder = builder
                .proxy(reqwest::Proxy::all(config.proxy_url.trim()).map_err(|_| "代理配置无效")?);
        }
        Ok(Self {
            http: builder.build().map_err(|_| "无法创建 ASR 连接")?,
            key,
            url: official_url(&config.endpoint, provider)?,
            provider,
        })
    }

    pub async fn recognize(&self, wav: Vec<u8>) -> Result<String, String> {
        if wav.len() > 220_000 || !wav.starts_with(b"RIFF") {
            return Err("ASR 音频切片格式或大小无效".into());
        }
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("caption.wav")
            .mime_str("audio/wav")
            .map_err(|_| "音频格式错误")?;
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if self.provider == CloudAsrProvider::Openai {
            form = form
                .text("model", "gpt-transcribe")
                .text("languages[]", "ja");
        }
        let response = self
            .http
            .post(self.url.clone())
            .bearer_auth(&self.key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "云端 ASR 超时，请检查网络或代理"
                } else {
                    "无法连接云端 ASR，请检查地址和代理"
                }
            })?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => "云端 ASR 认证失败，请检查关联密钥或账户权限".into(),
                402 => "云端 ASR 余额不足".into(),
                429 => "云端 ASR 达到速率或额度限制".into(),
                code => format!("云端 ASR 请求失败（HTTP {code}）"),
            });
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "接收 ASR 结果失败")?;
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("ASR 返回过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "ASR 返回无效 JSON")?;
        let text = body["text"].as_str().ok_or("ASR 结果缺少 text")?.trim();
        if text.contains(&self.key) {
            return Err("ASR 返回异常".into());
        }
        Ok(text.to_string())
    }
}

pub struct Audio {
    pub bytes: Vec<u8>,
    pub start_ms: u64,
    pub end_ms: u64,
}

pub async fn capture(
    state: &AppState,
    stop: &AtomicBool,
    tx: mpsc::Sender<Audio>,
) -> Result<(), String> {
    // Cloud mode must release an idle CUDA model before starting its capture-only worker.
    if let Some((_, worker)) = state.idle_asr_worker.lock().await.take() {
        worker.shutdown().await;
    }
    let mut worker = AsrWorker::spawn(&state.paths).await?;
    worker.start_cloud().await?;
    let mut stopping = None;
    let result = async {
        loop {
            if stop.load(Ordering::Relaxed) && stopping.is_none() {
                worker.stop().await?;
                stopping = Some(std::time::Instant::now());
            }
            if stopping.is_some_and(|t: std::time::Instant| t.elapsed() > Duration::from_secs(20)) {
                return Err("云端音频采集停止超时".into());
            }
            match tokio::time::timeout(Duration::from_millis(100), worker.next()).await {
                Ok(Ok(WorkerEvent::Audio {
                    wav_base64,
                    start_ms,
                    end_ms,
                })) => {
                    if wav_base64.len() > 300_000 {
                        return Err("音频切片过大".into());
                    }
                    let bytes = STANDARD
                        .decode(wav_base64)
                        .map_err(|_| "音频切片编码错误")?;
                    tx.try_send(Audio {
                        bytes,
                        start_ms,
                        end_ms,
                    })
                    .map_err(|_| "云端 ASR 持续落后，请检查网络或改用本地来源")?;
                }
                Ok(Ok(WorkerEvent::Health { detail, .. })) if detail == "capture_stopped" => break,
                Ok(Ok(WorkerEvent::Error { message, .. })) => return Err(message),
                Ok(Err(e)) => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }
    .await;
    worker.shutdown().await;
    result
}

// Apply exact deduplication only across real audio overlap. Do not fuzzy-delete new words.
pub fn trim_overlap(previous: &str, current: &str) -> String {
    let old: Vec<char> = previous.chars().filter(|c| c.is_alphanumeric()).collect();
    let positions: Vec<(usize, char)> = current
        .char_indices()
        .filter(|(_, c)| c.is_alphanumeric())
        .collect();
    let now: Vec<char> = positions.iter().map(|(_, c)| *c).collect();
    for n in (3..=6.min(old.len()).min(now.len().saturating_sub(2))).rev() {
        if old[old.len() - n..] == now[..n] {
            return current[positions[n].0..].trim().into();
        }
    }
    current.into()
}

#[tauri::command]
pub async fn test_cloud_asr(
    provider: CloudAsrProvider,
    config: CloudAsrConfig,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.read().map_err(|e| e.to_string())?.clone();
    let client = Client::new(&config, provider, &settings)?;
    // One second of silence validates the actual multipart ASR endpoint, not just /models.
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&32036u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&32000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&32000u32.to_le_bytes());
    wav.resize(32044, 0);
    let started = std::time::Instant::now();
    client.recognize(wav).await?;
    Ok(format!(
        "ASR 接口连接成功 · {}ms（静音测试，不代表识别精度）",
        started.elapsed().as_millis()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "uploads a short local fixture to both saved cloud ASR providers (billed)"]
    async fn saved_cloud_asr_smoke() {
        let paths = crate::settings::AppPaths::shared().unwrap();
        let mut settings = crate::settings::load(&paths).unwrap();
        for profile in &mut settings.llm_profiles {
            profile.api_key_configured =
                crate::secrets::profile_key(&profile.id).unwrap().is_some();
        }
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cache/cloud-asr-smoke.wav");
        let audio = std::fs::read(fixture).unwrap();
        for provider in [CloudAsrProvider::Openai, CloudAsrProvider::Deepseek] {
            let client = Client::new(
                settings.captions.cloud.config(provider),
                provider,
                &settings,
            )
            .unwrap();
            let started = std::time::Instant::now();
            let text = client.recognize(audio.clone()).await.unwrap();
            assert!(!text.is_empty(), "speech fixture should produce text");
            eprintln!(
                "{provider:?}: {}ms, {} characters",
                started.elapsed().as_millis(),
                text.chars().count()
            );
        }
    }
    #[test]
    fn recognition_binding_is_independent_of_translation_and_requires_valid_profile() {
        let mut settings = AppSettings::default();
        settings.llm.endpoint = "https://api.deepseek.com/v1".into();
        let config = CloudAsrConfig {
            profile_id: Some("speech".into()),
            endpoint: CloudAsrProvider::Openai.endpoint().into(),
            ..Default::default()
        };
        assert!(validate(&config, CloudAsrProvider::Openai, &settings).is_err());
        settings.llm_profiles.push(crate::models::LlmProfile {
            id: "speech".into(),
            name: "speech".into(),
            api_key_configured: true,
            config: crate::models::LlmProviderSettings {
                endpoint: "https://api.openai.com/v1".into(),
                ..Default::default()
            },
        });
        assert!(validate(&config, CloudAsrProvider::Openai, &settings).is_ok());
        assert!(validate(&config, CloudAsrProvider::Deepseek, &settings).is_err());
        let mut proxied = config.clone();
        proxied.proxy_enabled = true;
        assert!(validate(&proxied, CloudAsrProvider::Openai, &settings).is_err());
        proxied.proxy_url = "http://127.0.0.1:10808".into();
        assert!(validate(&proxied, CloudAsrProvider::Openai, &settings).is_ok());
        settings.llm_profiles[0].api_key_configured = false;
        assert!(validate(&config, CloudAsrProvider::Openai, &settings).is_err());
    }
    #[test]
    fn bound_credentials_stay_on_provider_origin() {
        assert!(official_url(
            "https://api.openai.com/v1/audio/transcriptions",
            CloudAsrProvider::Openai
        )
        .is_ok());
        for url in [
            "http://api.openai.com/v1",
            "https://api.deepseek.com/v1",
            "https://api.openai.com.evil.test/v1",
            "https://api.openai.com:444/v1",
        ] {
            assert!(official_url(url, CloudAsrProvider::Openai).is_err());
        }
    }
    #[test]
    fn overlap_preserves_new_words() {
        assert_eq!(trim_overlap("今日はいい天気", "いい天気ですね"), "ですね");
        assert_eq!(trim_overlap("今日は", "今日も楽しい"), "今日も楽しい");
    }
}
