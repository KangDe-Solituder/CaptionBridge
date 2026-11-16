const SERVICE: &str = "com.dimfi.livecaption";
const PRIMARY_API_KEY: &str = "primary-api-key";

/// Hold a possible key prefix across streaming chunks before publishing text.
pub struct ResponseKeyFilter<'a> {
    key: &'a str,
    pending: String,
}

impl<'a> ResponseKeyFilter<'a> {
    pub fn new(key: &'a str) -> Self {
        Self {
            key,
            pending: String::new(),
        }
    }

    pub fn push(&mut self, text: &str) -> String {
        if self.key.is_empty() {
            return text.to_string();
        }
        self.pending.push_str(text);
        self.pending = self.pending.replace(self.key, "[密钥已隐藏]");
        let split = self
            .pending
            .char_indices()
            .find(|(index, _)| self.key.starts_with(&self.pending[*index..]))
            .map_or(self.pending.len(), |(index, _)| index);
        let suffix = self.pending.split_off(split);
        std::mem::replace(&mut self.pending, suffix)
    }

    pub fn finish(self) -> String {
        self.pending
    }
}

pub fn normalize_api_key(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let key = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();

    if key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    if key.chars().any(char::is_whitespace) {
        return Err("API Key 不能包含空白字符".to_string());
    }

    Ok(key.to_string())
}

pub fn validate_credential_transport(endpoint: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "接口地址无效")?;
    if url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        Ok(())
    } else {
        Err(
            "使用 API Key 的远程接口必须使用 HTTPS；本机 localhost/127.0.0.1/::1 可使用 HTTP"
                .into(),
        )
    }
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, PRIMARY_API_KEY).map_err(|error| error.to_string())
}

pub fn get_api_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn has_api_key() -> bool {
    matches!(get_api_key(), Ok(Some(_)))
}

#[allow(dead_code)]
pub fn delete_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn profile_entry(id: &str) -> Result<keyring::Entry, String> {
    if id == "legacy" {
        return entry();
    }
    keyring::Entry::new(SERVICE, &format!("llm-profile-{id}")).map_err(|e| e.to_string())
}

pub fn profile_key(id: &str) -> Result<Option<String>, String> {
    match profile_entry(id)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn write_profile_key(id: &str, key: Option<&str>) -> Result<(), String> {
    let entry = profile_entry(id)?;
    match key {
        Some(key) => entry
            .set_password(&normalize_api_key(key)?)
            .map_err(|e| e.to_string()),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        },
    }
}

pub fn translation_key(settings: &crate::models::AppSettings) -> Result<String, String> {
    let id = settings
        .llm
        .profile_id
        .as_deref()
        .ok_or("请先在设置 → 应用中添加并启用翻译配置")?;
    let profile = settings
        .llm_profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or("当前翻译配置不存在，请重新选择")?;
    match profile_key(id)? {
        Some(key) => {
            validate_credential_transport(&settings.llm.endpoint)?;
            Ok(key)
        }
        None if !profile.api_key_configured => Ok(String::new()),
        None => Err("当前配置的密钥不可用，请编辑配置并重新保存 API Key".into()),
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use crate::models::{AppSettings, LlmProfile, LlmProviderSettings};

    #[test]
    fn credentials_require_tls_except_for_loopback_services() {
        for endpoint in [
            "https://example.com/v1",
            "http://localhost:1234/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:1234/v1",
        ] {
            assert!(validate_credential_transport(endpoint).is_ok());
        }
        for endpoint in [
            "http://example.com/v1",
            "http://localhost.example.com",
            "http://192.168.1.2:1234",
            "file:///key",
        ] {
            assert!(validate_credential_transport(endpoint).is_err());
        }
    }

    #[test]
    fn echoed_key_is_filtered_across_every_stream_boundary() {
        let key = "synthetic-test-credential";
        for boundary in 0..=key.len() {
            let mut filter = ResponseKeyFilter::new(key);
            let mut output = filter.push("服务返回：");
            output.push_str(&filter.push(&key[..boundary]));
            output.push_str(&filter.push(&key[boundary..]));
            output.push_str(&filter.push("。"));
            output.push_str(&filter.finish());
            assert_eq!(output, "服务返回：[密钥已隐藏]。");
            assert!(!output.contains(key));
        }
        let mut local = ResponseKeyFilter::new("");
        assert_eq!(local.push("正常译文"), "正常译文");
        let mut partial = ResponseKeyFilter::new(key);
        assert_eq!(partial.push("syn"), "");
        assert_eq!(partial.finish(), "syn");
    }

    #[test]
    #[ignore = "writes temporary isolated Windows credentials and removes them afterward"]
    fn profile_credentials_are_isolated_and_never_in_settings() {
        struct TemporaryKeys(Vec<String>);
        impl Drop for TemporaryKeys {
            fn drop(&mut self) {
                for id in &self.0 {
                    let _ = write_profile_key(id, None);
                }
            }
        }
        let ids = TemporaryKeys(vec![
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        ]);
        write_profile_key(&ids.0[0], Some("temporary-profile-test-one")).unwrap();
        write_profile_key(&ids.0[1], Some("temporary-profile-test-two")).unwrap();
        let mut settings = AppSettings::default();
        for id in &ids.0 {
            settings.llm_profiles.push(LlmProfile {
                id: id.clone(),
                name: "Test".into(),
                config: LlmProviderSettings {
                    profile_id: Some(id.clone()),
                    ..LlmProviderSettings::default()
                },
                api_key_configured: true,
            });
        }
        settings.llm = settings.llm_profiles[0].config.clone();
        assert_eq!(
            translation_key(&settings).unwrap(),
            "temporary-profile-test-one"
        );
        settings.llm = settings.llm_profiles[1].config.clone();
        assert_eq!(
            translation_key(&settings).unwrap(),
            "temporary-profile-test-two"
        );
        write_profile_key(&ids.0[1], None).unwrap();
        assert!(translation_key(&settings).is_err());
        settings.llm_profiles[1].api_key_configured = false;
        assert_eq!(translation_key(&settings).unwrap(), "");
        assert!(!serde_json::to_string(&settings)
            .unwrap()
            .contains("temporary-profile-test"));
    }
}
