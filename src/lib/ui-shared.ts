import { useEffect } from "react";
import type { AppSettings, TranslationResult } from "./contracts";

export const emptyResult: TranslationResult = { source_text: "", translated_text: "", model: "", latency_ms: 0, first_token_ms: 0, cached: false };

export const previewSettings: AppSettings = {
  schema_version: 9,
  llm: { endpoint: "https://api.openai.com/v1", model: "gpt-4o-mini", target_language: "中文", extra_body_json: "", timeout_milliseconds: 5000, max_tokens: 768, temperature: 0.2, thinking_enabled: false },
  selection: { enabled: true, clipboard_fallback_enabled: true, hotkey: "Alt+KeyQ", trigger_mode: "automatic" },
  captions: { source: { type: "local_asr", model_id: "kotoba-whisper-v2.0-faster", device: "cuda", compute_type: "int8_float16", vad_profile: "normal", channel_mode: "auto", channel_switch_sensitivity: "standard", suppress_non_speech_segments: true }, enabled: true, auto_launch: true, poll_milliseconds: 160, stable_milliseconds: 420, max_duration_milliseconds: 1800, max_chars: 96, context_segments: 2, audio_mode: "normal", model_mirror_url: "https://hf-mirror.com" },
  overlay: { opacity: .92, font_size: 22, width: 760, transparent: false, caption_color: "#ffffff", drag_mode: "alt" },
  visual: { theme: "system", blur_enabled: true, blur_scope: "floating", reduce_motion: false },
  downloads: { proxy_enabled: false, proxy_url: "" },
};

export function useTheme(settings?: AppSettings) {
  useEffect(() => {
    if (!settings) return;
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = settings.visual.theme === "system" ? (media.matches ? "dark" : "light") : settings.visual.theme;
      localStorage.setItem("livecaption-theme", settings.visual.theme);
      document.documentElement.dataset.theme = resolved;
      document.documentElement.dataset.blur = String(settings.visual.blur_enabled);
      document.documentElement.dataset.motion = settings.visual.reduce_motion ? "reduced" : "full";
    };
    apply(); media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings]);
}

export function displayHotkey(value: string) {
  return value.replace(/Key([A-Z])/g, "$1").replace(/Digit([0-9])/g, "$1");
}
