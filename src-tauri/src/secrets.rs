const SERVICE: &str = "com.dimfi.livecaption";
const PRIMARY_API_KEY: &str = "primary-api-key";

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
        Some(key) => Ok(key),
        None if !profile.api_key_configured => Ok(String::new()),
        None => Err("当前配置的密钥不可用，请编辑配置并重新保存 API Key".into()),
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use crate::models::{AppSettings, LlmProfile, LlmProviderSettings};

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
