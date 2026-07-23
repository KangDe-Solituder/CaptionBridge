use std::{fs, path::PathBuf};

use crate::models::AppSettings;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub settings_file: PathBuf,
    pub models_dir: PathBuf,
    pub downloads_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        let models_dir = data_dir.join("models");
        Self::new_with_models_dir(data_dir, models_dir)
    }

    fn new_with_models_dir(data_dir: PathBuf, models_dir: PathBuf) -> Self {
        Self {
            logs_dir: data_dir.join("logs"),
            sessions_dir: data_dir.join("sessions"),
            settings_file: data_dir.join("settings.json"),
            downloads_dir: models_dir.join(".downloads"),
            models_dir,
            data_dir,
        }
    }

    pub fn shared() -> Result<Self, String> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "Windows LOCALAPPDATA 路径不可用".to_string())?;
        let data_dir = local_app_data.join("com.dimfi.livecaption");
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = manifest_dir
            .parent()
            .ok_or_else(|| "无法确定应用工作区目录".to_string())?;
        Ok(Self::new_with_models_dir(
            data_dir,
            workspace_dir.join("Model"),
        ))
    }

    pub fn ensure(&self) -> Result<(), String> {
        fs::create_dir_all(&self.data_dir).map_err(|error| error.to_string())?;
        fs::create_dir_all(&self.logs_dir).map_err(|error| error.to_string())?;
        fs::create_dir_all(&self.sessions_dir)
            .map_err(|error| error.to_string())
            .and_then(|_| fs::create_dir_all(&self.models_dir).map_err(|error| error.to_string()))
            .and_then(|_| {
                fs::create_dir_all(&self.downloads_dir).map_err(|error| error.to_string())
            })
    }
}

pub fn load(paths: &AppPaths) -> Result<AppSettings, String> {
    paths.ensure()?;
    if !paths.settings_file.exists() {
        return Ok(AppSettings::default());
    }

    let raw = fs::read_to_string(&paths.settings_file).map_err(|error| error.to_string())?;
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    let previous_schema = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if previous_schema < 6 {
        let captions = value
            .get_mut("captions")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| "设置文件缺少 captions".to_string())?;
        captions.insert(
            "source".to_string(),
            serde_json::json!({ "type": "windows_live_caption" }),
        );
        captions
            .entry("audio_mode".to_string())
            .or_insert(serde_json::json!("normal"));
    }
    let mut settings: AppSettings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.schema_version = AppSettings::default().schema_version;
    if settings.selection.hotkey == "Alt+Q" {
        settings.selection.hotkey = "Alt+KeyQ".to_string();
    }
    if previous_schema < 4 {
        settings.llm.timeout_milliseconds = settings.llm.timeout_milliseconds.min(5_000);
        settings.captions.poll_milliseconds = 160;
        settings.captions.stable_milliseconds = 420;
        settings.captions.max_duration_milliseconds = 1_800;
    }
    if previous_schema < 5 {
        settings.captions.context_segments = 2;
    }
    if crate::windows_integration::validate_hotkey(&settings.selection.hotkey).is_err() {
        settings.selection.hotkey = AppSettings::default().selection.hotkey;
    }
    Ok(settings)
}

pub fn save(paths: &AppPaths, settings: &AppSettings) -> Result<(), String> {
    paths.ensure()?;
    let raw = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&paths.settings_file, raw).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AudioChannelMode, ChannelSwitchSensitivity, OverlayDragMode};

    #[test]
    fn settings_roundtrip_preserves_release_sensitive_values() {
        let test_dir =
            std::env::temp_dir().join(format!("livecaption-settings-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::new(test_dir.clone());
        let mut expected = AppSettings::default();
        expected.llm.endpoint = "https://api.deepseek.com/v1".to_string();
        expected.llm.model = "deepseek-v4-flash".to_string();
        expected.overlay.drag_mode = OverlayDragMode::Anywhere;
        expected.overlay.caption_color = "#c8a6ff".to_string();

        save(&paths, &expected).unwrap();
        let actual = load(&paths).unwrap();
        assert_eq!(actual.llm.endpoint, expected.llm.endpoint);
        assert_eq!(actual.llm.model, expected.llm.model);
        assert_eq!(actual.overlay.drag_mode, OverlayDragMode::Anywhere);
        assert_eq!(actual.overlay.caption_color, "#c8a6ff");

        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn shared_path_is_stable_across_build_profiles() {
        let paths = AppPaths::shared().unwrap();
        let expected_models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Model");
        assert!(paths.data_dir.ends_with("com.dimfi.livecaption"));
        assert_eq!(paths.models_dir, expected_models_dir);
        assert_eq!(paths.downloads_dir, paths.models_dir.join(".downloads"));
    }

    #[test]
    fn new_install_defaults_to_kotoba() {
        let settings = AppSettings::default();
        assert_eq!(settings.schema_version, 8);
        assert_eq!(
            settings.captions.source.model_id(),
            Some("kotoba-whisper-v2.0-faster")
        );
        assert_eq!(
            settings.captions.source.channel_mode(),
            Some(AudioChannelMode::Auto)
        );
        assert_eq!(
            settings.captions.source.channel_switch_sensitivity(),
            Some(ChannelSwitchSensitivity::Standard)
        );
        assert_eq!(
            settings.captions.source.suppress_non_speech_segments(),
            Some(true)
        );
    }

    #[test]
    fn v5_migrates_explicitly_to_windows_source() {
        let test_dir =
            std::env::temp_dir().join(format!("livecaption-v5-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::new(test_dir.clone());
        paths.ensure().unwrap();
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        value["schema_version"] = serde_json::json!(5);
        value["captions"].as_object_mut().unwrap().remove("source");
        value["captions"]
            .as_object_mut()
            .unwrap()
            .remove("audio_mode");
        fs::write(&paths.settings_file, serde_json::to_vec(&value).unwrap()).unwrap();

        let migrated = load(&paths).unwrap();
        assert!(matches!(
            migrated.captions.source,
            crate::models::CaptionSourceConfig::WindowsLiveCaption
        ));
        assert_eq!(migrated.schema_version, 8);
        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn v6_defaults_to_automatic_asmr_channel_routing() {
        let test_dir =
            std::env::temp_dir().join(format!("livecaption-v6-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::new(test_dir.clone());
        paths.ensure().unwrap();
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        value["schema_version"] = serde_json::json!(6);
        value["captions"]["source"]
            .as_object_mut()
            .unwrap()
            .remove("channel_mode");
        fs::write(&paths.settings_file, serde_json::to_vec(&value).unwrap()).unwrap();

        let migrated = load(&paths).unwrap();
        assert_eq!(
            migrated.captions.source.channel_mode(),
            Some(AudioChannelMode::Auto)
        );
        assert_eq!(migrated.schema_version, 8);
        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn v7_defaults_to_standard_sticky_routing() {
        let test_dir =
            std::env::temp_dir().join(format!("livecaption-v7-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::new(test_dir.clone());
        paths.ensure().unwrap();
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        value["schema_version"] = serde_json::json!(7);
        let source = value["captions"]["source"].as_object_mut().unwrap();
        source.remove("channel_switch_sensitivity");
        source.remove("suppress_non_speech_segments");
        fs::write(&paths.settings_file, serde_json::to_vec(&value).unwrap()).unwrap();

        let migrated = load(&paths).unwrap();
        assert_eq!(
            migrated.captions.source.channel_switch_sensitivity(),
            Some(ChannelSwitchSensitivity::Standard)
        );
        assert_eq!(
            migrated.captions.source.suppress_non_speech_segments(),
            Some(true)
        );
        assert_eq!(migrated.schema_version, 8);
        let _ = fs::remove_dir_all(test_dir);
    }
}
