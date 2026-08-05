use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

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
        let local_models_dir = data_dir.join("models");
        let configured_models_dir = std::env::var_os("LIVECAPTION_MODEL_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let legacy_models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|workspace| workspace.join("Model"))
            .filter(|path| path.is_dir());
        let models_dir = configured_models_dir
            .or(legacy_models_dir)
            .unwrap_or(local_models_dir);
        Ok(Self::new_with_models_dir(data_dir, models_dir))
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
    normalize(&mut settings);
    Ok(settings)
}

pub fn save(paths: &AppPaths, settings: &AppSettings) -> Result<(), String> {
    paths.ensure()?;
    let mut normalized = settings.clone();
    normalize(&mut normalized);
    let raw = serde_json::to_vec_pretty(&normalized).map_err(|error| error.to_string())?;
    let temporary = paths.settings_file.with_extension("json.tmp");
    let backup = paths.settings_file.with_extension("json.bak");
    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    output.write_all(&raw).map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    drop(output);

    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| error.to_string())?;
    }
    if paths.settings_file.exists() {
        fs::rename(&paths.settings_file, &backup)
            .map_err(|error| format!("无法备份旧设置：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &paths.settings_file) {
        if backup.exists() {
            let _ = fs::rename(&backup, &paths.settings_file);
        }
        return Err(format!("无法保存设置：{error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

pub fn normalize(settings: &mut AppSettings) {
    let defaults = AppSettings::default();
    settings.schema_version = defaults.schema_version;
    settings.llm.endpoint = settings
        .llm
        .endpoint
        .trim()
        .trim_end_matches('/')
        .to_string();
    if settings.llm.endpoint.is_empty() {
        settings.llm.endpoint = defaults.llm.endpoint;
    }
    settings.llm.model = settings.llm.model.trim().to_string();
    if settings.llm.model.is_empty() {
        settings.llm.model = defaults.llm.model;
    }
    settings.llm.target_language = settings.llm.target_language.trim().to_string();
    if settings.llm.target_language.is_empty() {
        settings.llm.target_language = defaults.llm.target_language;
    }
    settings.llm.timeout_milliseconds = settings.llm.timeout_milliseconds.clamp(500, 5_000);
    settings.llm.max_tokens = settings.llm.max_tokens.clamp(1, 4_096);
    settings.llm.temperature = if settings.llm.temperature.is_finite() {
        settings.llm.temperature.clamp(0.0, 2.0)
    } else {
        defaults.llm.temperature
    };
    settings.captions.poll_milliseconds = settings.captions.poll_milliseconds.clamp(80, 2_000);
    settings.captions.stable_milliseconds =
        settings.captions.stable_milliseconds.clamp(100, 10_000);
    settings.captions.max_duration_milliseconds = settings
        .captions
        .max_duration_milliseconds
        .clamp(500, 30_000);
    settings.captions.max_chars = settings.captions.max_chars.clamp(16, 512);
    settings.captions.context_segments = settings.captions.context_segments.min(4);
    settings.captions.model_mirror_url = settings
        .captions
        .model_mirror_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    settings.overlay.opacity = if settings.overlay.opacity.is_finite() {
        settings.overlay.opacity.clamp(0.2, 1.0)
    } else {
        defaults.overlay.opacity
    };
    settings.overlay.font_size = settings.overlay.font_size.clamp(14, 72);
    settings.overlay.width = settings.overlay.width.clamp(320, 1_920);
    if !is_hex_color(&settings.overlay.caption_color) {
        settings.overlay.caption_color = defaults.overlay.caption_color;
    }
    if crate::windows_integration::validate_hotkey(&settings.selection.hotkey).is_err() {
        settings.selection.hotkey = defaults.selection.hotkey;
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
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
        let legacy_models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Model");
        assert!(paths.data_dir.ends_with("com.dimfi.livecaption"));
        if legacy_models_dir.is_dir() {
            assert_eq!(paths.models_dir, legacy_models_dir);
        } else {
            assert_eq!(paths.models_dir, paths.data_dir.join("models"));
        }
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
    fn normalization_clamps_unsafe_manual_values() {
        let mut settings = AppSettings::default();
        settings.llm.endpoint = "  ".to_string();
        settings.llm.max_tokens = u32::MAX;
        settings.captions.poll_milliseconds = 0;
        settings.captions.max_chars = usize::MAX;
        settings.overlay.opacity = f32::NAN;
        settings.overlay.caption_color = "transparent".to_string();

        normalize(&mut settings);

        assert_eq!(settings.llm.endpoint, AppSettings::default().llm.endpoint);
        assert_eq!(settings.llm.max_tokens, 4_096);
        assert_eq!(settings.captions.poll_milliseconds, 80);
        assert_eq!(settings.captions.max_chars, 512);
        assert_eq!(
            settings.overlay.opacity,
            AppSettings::default().overlay.opacity
        );
        assert_eq!(settings.overlay.caption_color, "#ffffff");
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
