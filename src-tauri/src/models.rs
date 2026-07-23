use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrDependencyStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub detail: String,
    pub detected_path: Option<String>,
    pub official_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrDependencyReport {
    pub ready: bool,
    pub dependencies: Vec<AsrDependencyStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationMode {
    Selection,
    LiveCaption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderSettings {
    pub endpoint: String,
    pub model: String,
    pub target_language: String,
    pub extra_body_json: String,
    pub timeout_milliseconds: u64,
    pub max_tokens: u32,
    pub temperature: f32,
    #[serde(default)]
    pub thinking_enabled: bool,
}

impl Default for LlmProviderSettings {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            target_language: "中文".to_string(),
            extra_body_json: String::new(),
            timeout_milliseconds: 5_000,
            max_tokens: 768,
            temperature: 0.2,
            thinking_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionSettings {
    pub enabled: bool,
    pub clipboard_fallback_enabled: bool,
    #[serde(default)]
    pub trigger_mode: SelectionTriggerMode,
    #[serde(default = "default_selection_hotkey")]
    pub hotkey: String,
}

impl Default for SelectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            clipboard_fallback_enabled: true,
            trigger_mode: SelectionTriggerMode::Hotkey,
            hotkey: default_selection_hotkey(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionTriggerMode {
    #[default]
    Hotkey,
    Automatic,
}

fn default_selection_hotkey() -> String {
    "Alt+KeyQ".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionSettings {
    #[serde(default)]
    pub source: CaptionSourceConfig,
    pub enabled: bool,
    pub auto_launch: bool,
    pub poll_milliseconds: u64,
    pub stable_milliseconds: u64,
    pub max_duration_milliseconds: u64,
    pub max_chars: usize,
    #[serde(default = "default_caption_context_segments")]
    pub context_segments: usize,
    #[serde(default)]
    pub audio_mode: VadProfile,
    #[serde(default = "default_model_mirror")]
    pub model_mirror_url: String,
}

impl Default for CaptionSettings {
    fn default() -> Self {
        Self {
            source: CaptionSourceConfig::default(),
            enabled: true,
            auto_launch: true,
            poll_milliseconds: 160,
            stable_milliseconds: 420,
            max_duration_milliseconds: 1800,
            max_chars: 96,
            context_segments: 2,
            audio_mode: VadProfile::Normal,
            model_mirror_url: default_model_mirror(),
        }
    }
}

fn default_model_mirror() -> String {
    "https://hf-mirror.com".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptionSourceConfig {
    WindowsLiveCaption,
    LocalAsr {
        model_id: String,
        #[serde(default = "default_asr_device")]
        device: String,
        #[serde(default = "default_compute_type")]
        compute_type: String,
        #[serde(default)]
        vad_profile: VadProfile,
        #[serde(default)]
        channel_mode: AudioChannelMode,
        #[serde(default)]
        channel_switch_sensitivity: ChannelSwitchSensitivity,
        #[serde(default = "default_true")]
        suppress_non_speech_segments: bool,
    },
}

impl Default for CaptionSourceConfig {
    fn default() -> Self {
        Self::LocalAsr {
            model_id: "kotoba-whisper-v2.0-faster".to_string(),
            device: default_asr_device(),
            compute_type: default_compute_type(),
            vad_profile: VadProfile::Normal,
            channel_mode: AudioChannelMode::Auto,
            channel_switch_sensitivity: ChannelSwitchSensitivity::Standard,
            suppress_non_speech_segments: true,
        }
    }
}

impl CaptionSourceConfig {
    pub fn kind(&self) -> CaptionSourceKind {
        match self {
            Self::WindowsLiveCaption => CaptionSourceKind::WindowsLiveCaption,
            Self::LocalAsr { .. } => CaptionSourceKind::LocalAsr,
        }
    }

    pub fn model_id(&self) -> Option<&str> {
        match self {
            Self::WindowsLiveCaption => None,
            Self::LocalAsr { model_id, .. } => Some(model_id),
        }
    }

    pub fn channel_mode(&self) -> Option<AudioChannelMode> {
        match self {
            Self::WindowsLiveCaption => None,
            Self::LocalAsr { channel_mode, .. } => Some(*channel_mode),
        }
    }

    pub fn channel_switch_sensitivity(&self) -> Option<ChannelSwitchSensitivity> {
        match self {
            Self::WindowsLiveCaption => None,
            Self::LocalAsr {
                channel_switch_sensitivity,
                ..
            } => Some(*channel_switch_sensitivity),
        }
    }

    pub fn suppress_non_speech_segments(&self) -> Option<bool> {
        match self {
            Self::WindowsLiveCaption => None,
            Self::LocalAsr {
                suppress_non_speech_segments,
                ..
            } => Some(*suppress_non_speech_segments),
        }
    }
}

fn default_asr_device() -> String {
    "cuda".to_string()
}

fn default_compute_type() -> String {
    "int8_float16".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadProfile {
    #[default]
    Normal,
    Asmr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioChannelMode {
    Mono,
    #[default]
    Auto,
    Mix,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSwitchSensitivity {
    Stable,
    #[default]
    Standard,
    Responsive,
}

impl VadProfile {
    pub fn parameters(
        self,
        requested_channel_mode: AudioChannelMode,
        channel_switch_sensitivity: ChannelSwitchSensitivity,
        suppress_non_speech_segments: bool,
    ) -> VadParameters {
        match self {
            Self::Normal => VadParameters {
                threshold: 0.5,
                silence_commit_ms: 500,
                max_segment_ms: 8_000,
                partial_interval_ms: 800,
                channel_mode: AudioChannelMode::Mono,
                channel_switch_sensitivity: ChannelSwitchSensitivity::Standard,
                suppress_non_speech_segments: false,
            },
            Self::Asmr => VadParameters {
                threshold: 0.3,
                silence_commit_ms: 1_000,
                max_segment_ms: 12_000,
                partial_interval_ms: 1_200,
                channel_mode: match requested_channel_mode {
                    AudioChannelMode::Mono => AudioChannelMode::Auto,
                    mode => mode,
                },
                channel_switch_sensitivity,
                suppress_non_speech_segments,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct VadParameters {
    pub threshold: f32,
    pub silence_commit_ms: u64,
    pub max_segment_ms: u64,
    pub partial_interval_ms: u64,
    pub channel_mode: AudioChannelMode,
    pub channel_switch_sensitivity: ChannelSwitchSensitivity,
    pub suppress_non_speech_segments: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSourceKind {
    WindowsLiveCaption,
    LocalAsr,
}

fn default_caption_context_segments() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlaySettings {
    pub opacity: f32,
    pub font_size: u32,
    pub width: u32,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default = "default_caption_color")]
    pub caption_color: String,
    #[serde(default)]
    pub drag_mode: OverlayDragMode,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            opacity: 0.92,
            font_size: 22,
            width: 760,
            transparent: false,
            caption_color: default_caption_color(),
            drag_mode: OverlayDragMode::Alt,
            x: None,
            y: None,
        }
    }
}

fn default_caption_color() -> String {
    "#ffffff".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayDragMode {
    #[default]
    Alt,
    Anywhere,
    Handle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlurScope {
    Floating,
    All,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualSettings {
    pub theme: ThemeMode,
    pub blur_enabled: bool,
    pub blur_scope: BlurScope,
    pub reduce_motion: bool,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            blur_enabled: true,
            blur_scope: BlurScope::Floating,
            reduce_motion: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub schema_version: u32,
    pub llm: LlmProviderSettings,
    pub selection: SelectionSettings,
    pub captions: CaptionSettings,
    pub overlay: OverlaySettings,
    pub visual: VisualSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 8,
            llm: LlmProviderSettings::default(),
            selection: SelectionSettings::default(),
            captions: CaptionSettings::default(),
            overlay: OverlaySettings::default(),
            visual: VisualSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub settings: AppSettings,
    pub api_key_configured: bool,
    pub model_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSaveRequest {
    pub settings: AppSettings,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRequest {
    pub mode: TranslationMode,
    pub source_text: String,
    pub source_language: String,
    pub target_language: String,
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    pub source_text: String,
    pub translated_text: String,
    pub model: String,
    pub latency_ms: u128,
    pub first_token_ms: u128,
    pub cached: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionSegment {
    pub id: String,
    pub source_text: String,
    pub revision: u64,
    pub committed: bool,
    #[serde(default = "default_segment_source")]
    pub source: CaptionSourceKind,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: u64,
    #[serde(default = "default_true")]
    pub final_result: bool,
    #[serde(default)]
    pub time_source: CaptionTimeSource,
}

fn default_segment_source() -> CaptionSourceKind {
    CaptionSourceKind::WindowsLiveCaption
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaptionTimeSource {
    Model,
    #[default]
    Estimated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionTranslatedEvent {
    pub segment: CaptionSegment,
    pub result: TranslationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionReadyEvent {
    pub text: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub caption_running: bool,
    pub caption_state: CaptionRuntimeState,
    pub selection_hotkey_registered: bool,
    pub last_error: Option<String>,
    pub source_health: CaptionSourceHealth,
    pub last_caption_update_ms: u64,
    pub reader_restarts: u32,
    pub source_error_count: u32,
    pub translation_queue_depth: u32,
    pub last_translation_latency_ms: u64,
    pub last_translation_first_token_ms: u64,
    pub selected_source: CaptionSourceConfig,
    pub active_source: Option<CaptionSourceConfig>,
    pub active_model: Option<String>,
    pub asr_latency_ms: u64,
    pub asmr_channel_state: AsmrChannelState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AsmrChannelState {
    #[default]
    Inactive,
    Searching,
    LockedLeft,
    LockedRight,
    LockedMix,
    PendingLeft,
    PendingRight,
    FallbackMono,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSourceHealth {
    Unknown,
    Ready,
    Quiet,
    StatusPrompt,
    Stale,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CaptionRuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Switching,
    LoadingModel,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    NotInstalled,
    Downloading,
    Verifying,
    Available,
    Loading,
    Active,
    Corrupt,
    Incompatible,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub repository: String,
    pub revision: String,
    pub license: String,
    pub expected_size_bytes: u64,
    pub installed_size_bytes: u64,
    pub status: ModelStatus,
    pub downloaded_bytes: u64,
    pub error: Option<String>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProgressEvent {
    pub model_id: String,
    pub status: ModelStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionExportEntry {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDescriptor {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}
