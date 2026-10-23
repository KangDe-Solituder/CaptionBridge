use crate::{
    models::{AppSettings, LlmProfile, LlmProviderSettings, SettingsView},
    secrets, settings, AppState,
};
use serde::Deserialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

#[derive(Deserialize)]
pub struct ProfileInput {
    pub id: Option<String>,
    pub name: String,
    pub config: LlmProviderSettings,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

pub fn migrate_legacy(settings: &mut AppSettings, has_key: bool) {
    let defaults = LlmProviderSettings::default();
    if settings.llm_profiles.is_empty()
        && settings.llm.profile_id.is_none()
        && (has_key
            || settings.llm.endpoint != defaults.endpoint
            || settings.llm.model != defaults.model)
    {
        settings.llm.profile_id = Some("legacy".into());
        settings.llm_profiles.push(LlmProfile {
            id: "legacy".into(),
            name: "原有配置".into(),
            config: settings.llm.clone(),
            api_key_configured: has_key,
        });
    }
}

fn validate(input: &mut ProfileInput, previous: Option<&LlmProfile>) -> Result<(), String> {
    input.name = input.name.trim().to_string();
    input.config.endpoint = input
        .config
        .endpoint
        .trim()
        .trim_end_matches('/')
        .to_string();
    input.config.model = input.config.model.trim().to_string();
    if input.name.is_empty() || input.name.chars().count() > 80 {
        return Err("配置名称需为 1–80 个字符".into());
    }
    if input.config.model.is_empty() {
        return Err("请填写模型名称".into());
    }
    let url = reqwest::Url::parse(&input.config.endpoint).map_err(|_| "请填写有效的接口地址")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("接口地址需为 HTTP(S) 地址，不能包含用户名、密码、查询参数或片段".into());
    }
    if !input.config.extra_body_json.trim().is_empty() {
        let body: serde_json::Value = serde_json::from_str(&input.config.extra_body_json)
            .map_err(|_| "Extra Body JSON 格式无效")?;
        if !body.is_object() {
            return Err("Extra Body JSON 必须是对象".into());
        }
    }
    let new_key = input
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty());
    if input.clear_api_key && new_key.is_some() {
        return Err("请在填写新密钥与清除密钥之间选择一种操作".into());
    }
    if let Some(key) = new_key {
        secrets::normalize_api_key(key)?;
    }
    if previous.is_some_and(|p| {
        p.api_key_configured && p.config.endpoint.trim_end_matches('/') != input.config.endpoint
    }) && new_key.is_none()
        && !input.clear_api_key
    {
        return Err("接口地址已更改，请重新填写 API Key，或勾选清除已保存的密钥".into());
    }
    // Reuse existing numeric and language normalization.
    let mut normalized = AppSettings {
        llm: input.config.clone(),
        ..AppSettings::default()
    };
    settings::normalize(&mut normalized);
    input.config = normalized.llm;
    Ok(())
}

fn publish(state: &AppState, app: &AppHandle, settings: AppSettings) -> SettingsView {
    let configured = settings
        .llm_profiles
        .iter()
        .any(|p| Some(&p.id) == settings.llm.profile_id.as_ref() && p.api_key_configured);
    state
        .api_key_configured
        .store(configured, Ordering::Relaxed);
    let view = SettingsView {
        settings,
        api_key_configured: configured,
        model_directory: state.paths.models_dir.to_string_lossy().into_owned(),
    };
    let _ = app.emit("llm:profiles-changed", &view);
    view
}

#[tauri::command]
pub async fn save_llm_profile(
    mut input: ProfileInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let mut current = state.settings.write().map_err(|e| e.to_string())?;
    let mut previous = input
        .id
        .as_ref()
        .map(|id| {
            current
                .llm_profiles
                .iter()
                .find(|p| &p.id == id)
                .cloned()
                .ok_or("配置已被移除，请重新添加")
        })
        .transpose()?;
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let old_key = secrets::profile_key(&id)?;
    if let Some(profile) = previous.as_mut() {
        profile.api_key_configured |= old_key.is_some();
        if profile.api_key_configured
            && old_key.is_none()
            && input
                .api_key
                .as_deref()
                .is_none_or(|key| key.trim().is_empty())
            && !input.clear_api_key
        {
            return Err("原密钥不可用，请重新填写 API Key 或明确清除密钥".into());
        }
    }
    validate(&mut input, previous.as_ref())?;
    let key = if input.clear_api_key {
        None
    } else {
        input
            .api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| old_key.clone())
    };
    input.config.profile_id = Some(id.clone());
    let profile = LlmProfile {
        id: id.clone(),
        name: input.name,
        config: input.config,
        api_key_configured: key.is_some(),
    };
    let mut next = current.clone();
    if let Some(slot) = next.llm_profiles.iter_mut().find(|p| p.id == id) {
        *slot = profile.clone();
    } else {
        next.llm_profiles.push(profile.clone());
    }
    if current.llm_profiles.is_empty() || next.llm.profile_id.as_ref() == Some(&id) {
        next.llm = profile.config;
    }
    secrets::write_profile_key(&id, key.as_deref())?;
    if let Err(error) = settings::save(&state.paths, &next) {
        secrets::write_profile_key(&id, old_key.as_deref())
            .map_err(|_| "配置保存失败，凭据回滚也失败，请重新编辑该配置")?;
        return Err(error);
    }
    *current = next.clone();
    Ok(publish(&state, &app, next))
}

#[tauri::command]
pub async fn activate_llm_profile(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let mut current = state.settings.write().map_err(|e| e.to_string())?;
    let profile = current
        .llm_profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or("配置不存在")?;
    let mut next = current.clone();
    next.llm = profile.config.clone();
    settings::save(&state.paths, &next)?;
    *current = next.clone();
    Ok(publish(&state, &app, next))
}

#[tauri::command]
pub async fn remove_llm_profile(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let mut current = state.settings.write().map_err(|e| e.to_string())?;
    if !current.llm_profiles.iter().any(|p| p.id == id) {
        return Err("配置不存在".into());
    }
    let old_key = secrets::profile_key(&id)?;
    let mut next = current.clone();
    next.llm_profiles.retain(|p| p.id != id);
    if next.llm.profile_id.as_ref() == Some(&id) {
        next.llm = LlmProviderSettings::default();
    }
    secrets::write_profile_key(&id, None)?;
    if let Err(error) = settings::save(&state.paths, &next) {
        secrets::write_profile_key(&id, old_key.as_deref())
            .map_err(|_| "移除失败，凭据回滚也失败，请重新编辑该配置")?;
        return Err(error);
    }
    *current = next.clone();
    Ok(publish(&state, &app, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ProfileInput {
        ProfileInput {
            id: None,
            name: "模型配置".into(),
            config: LlmProviderSettings::default(),
            api_key: None,
            clear_api_key: false,
        }
    }

    #[test]
    fn empty_install_stays_empty_and_legacy_migration_is_idempotent() {
        let mut settings = AppSettings::default();
        migrate_legacy(&mut settings, false);
        assert!(settings.llm_profiles.is_empty());
        migrate_legacy(&mut settings, true);
        migrate_legacy(&mut settings, true);
        assert_eq!(settings.llm_profiles.len(), 1);
        assert_eq!(settings.llm.profile_id.as_deref(), Some("legacy"));
        assert_eq!(settings.llm_profiles[0].name, "原有配置");
        assert!(settings.llm_profiles[0].api_key_configured);
    }

    #[test]
    fn custom_legacy_local_service_is_preserved_without_a_key() {
        let mut settings = AppSettings::default();
        settings.llm.endpoint = "http://localhost:1234/v1".into();
        migrate_legacy(&mut settings, false);
        assert_eq!(
            settings.llm_profiles[0].config.endpoint,
            settings.llm.endpoint
        );
        assert!(!settings.llm_profiles[0].api_key_configured);
    }

    #[test]
    fn endpoint_change_requires_replacing_or_explicitly_clearing_the_key() {
        let previous = LlmProfile {
            id: "id".into(),
            name: "saved".into(),
            config: LlmProviderSettings::default(),
            api_key_configured: true,
        };
        let mut edit = input();
        assert!(validate(&mut edit, Some(&previous)).is_ok());
        edit.config.endpoint = "https://another.example/v1".into();
        assert!(validate(&mut edit, Some(&previous)).is_err());
        edit.clear_api_key = true;
        assert!(validate(&mut edit, Some(&previous)).is_ok());
        edit.clear_api_key = false;
        edit.api_key = Some("new-test-key".into());
        assert!(validate(&mut edit, Some(&previous)).is_ok());
    }

    #[test]
    fn profile_validation_rejects_invalid_endpoint_and_extra_body() {
        for endpoint in [
            "file:///tmp/config",
            "https://user:secret@example.com",
            "https://example.com?api_key=secret",
            "invalid",
        ] {
            let mut draft = input();
            draft.config.endpoint = endpoint.into();
            assert!(validate(&mut draft, None).is_err());
        }
        let mut draft = input();
        draft.config.extra_body_json = "[]".into();
        assert!(validate(&mut draft, None).is_err());
    }

    #[test]
    fn settings_roundtrip_keeps_profiles_and_active_selection() {
        let mut settings = AppSettings::default();
        migrate_legacy(&mut settings, true);
        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.llm.profile_id, settings.llm.profile_id);
        assert_eq!(
            decoded.llm_profiles[0].config.endpoint,
            settings.llm.endpoint
        );
        assert!(!encoded.contains("primary-api-key"));
        assert!(serde_json::to_value(&decoded.llm_profiles[0])
            .unwrap()
            .get("api_key")
            .is_none());
    }
}
