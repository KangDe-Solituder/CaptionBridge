export type TranslationMode = "selection" | "live_caption";
export type CaptionRuntimeState = "stopped" | "starting" | "running" | "stopping" | "switching" | "loading_model" | "error";
export type CaptionSourceHealth = "unknown" | "ready" | "quiet" | "status_prompt" | "stale" | "reconnecting" | "error";

export interface LlmProviderSettings {
  endpoint: string; model: string; target_language: string; extra_body_json: string;
  timeout_milliseconds: number; max_tokens: number; temperature: number; thinking_enabled: boolean;
}
export interface SelectionSettings { enabled: boolean; clipboard_fallback_enabled: boolean; hotkey: string; trigger_mode: "hotkey" | "automatic"; }
export interface CaptionSettings {
  source: CaptionSourceConfig;
  enabled: boolean; auto_launch: boolean; poll_milliseconds: number;
  stable_milliseconds: number; max_duration_milliseconds: number; max_chars: number; context_segments: number;
  audio_mode: "normal" | "asmr"; model_mirror_url: string;
}
export type CaptionSourceConfig =
  | { type: "windows_live_caption" }
  | { type: "local_asr"; model_id: string; device: string; compute_type: string; vad_profile: "normal" | "asmr" };
export interface OverlaySettings {
  opacity: number; font_size: number; width: number; x?: number; y?: number;
  transparent: boolean; caption_color: string; drag_mode: "alt" | "anywhere" | "handle";
}
export interface VisualSettings {
  theme: "system" | "light" | "dark"; blur_enabled: boolean;
  blur_scope: "floating" | "all" | "off"; reduce_motion: boolean;
}
export interface AppSettings {
  schema_version: number; llm: LlmProviderSettings; selection: SelectionSettings;
  captions: CaptionSettings; overlay: OverlaySettings; visual: VisualSettings;
}
export interface SettingsView { settings: AppSettings; api_key_configured: boolean; model_directory: string; }
export interface TranslationRequest {
  mode: TranslationMode; source_text: string; source_language: string; target_language: string; context?: string[];
}
export interface TranslationResult {
  source_text: string; translated_text: string; model: string; latency_ms: number; first_token_ms: number; cached: boolean; error?: string;
}
export interface CaptionSegment {
  id: string; source_text: string; revision: number; committed: boolean;
  source: "windows_live_caption" | "local_asr"; model_id?: string;
  start_ms: number; end_ms: number; final_result: boolean; time_source: "model" | "estimated";
}
export interface RuntimeStatus {
  caption_running: boolean; caption_state: CaptionRuntimeState;
  selection_hotkey_registered: boolean; last_error?: string; source_health: CaptionSourceHealth;
  last_caption_update_ms: number; reader_restarts: number; source_error_count: number;
  translation_queue_depth: number; last_translation_latency_ms: number; last_translation_first_token_ms: number;
  selected_source: CaptionSourceConfig; active_source?: CaptionSourceConfig; active_model?: string; asr_latency_ms: number;
}
export interface SelectionReadyEvent { text: string; x: number; y: number; }
export interface CaptionTranslatedEvent { segment: CaptionSegment; result: TranslationResult; }
export interface RuntimeLogEntry { timestamp: string; level: string; message: string; }
export type ModelStatus = "not_installed" | "downloading" | "verifying" | "available" | "loading" | "active" | "corrupt" | "incompatible" | "failed";
export interface ModelInfo {
  id: string; display_name: string; repository: string; revision: string; license: string;
  expected_size_bytes: number; installed_size_bytes: number; status: ModelStatus;
  downloaded_bytes: number; error?: string; recommended: boolean;
}
export interface ModelProgressEvent {
  model_id: string; status: ModelStatus; downloaded_bytes: number; total_bytes: number; error?: string;
}
