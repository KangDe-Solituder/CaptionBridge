import type { AppSettings, CaptionSourceConfig, ModelInfo, TranslationMode } from "./contracts";

export function translationRoutes(s: AppSettings): Record<TranslationMode, string | null> {
  const first = s.llm_profiles[0]?.id ?? null;
  const legacy = s.llm_profiles.some(p => p.id === s.llm.profile_id) ? s.llm.profile_id! : first;
  const routes = s.translation_profiles ?? { selection: legacy, live_caption: legacy, screenshot: legacy };
  return Object.fromEntries(Object.entries(routes).map(([mode, id]) => [mode, s.llm_profiles.some(p => p.id === id) ? id : first])) as Record<TranslationMode, string | null>;
}
export function officialEndpoint(value: string, provider: string) {
  try { const u = new URL(value); return u.protocol === "https:" && u.hostname === `api.${provider}.com` && (!u.port || u.port === "443") && !u.username && !u.password && !u.search && !u.hash; } catch { return false; }
}
export function cloudReady(s: AppSettings, kind: "asr" | "ocr", provider: "openai" | "deepseek") {
  const config = kind === "asr" ? s.captions.cloud[provider] : { profile_id: s.ocr.cloud_profile_id, endpoint: s.ocr.cloud_endpoint };
  if ("proxy_enabled" in config && config.proxy_enabled) {
    try {
      const u = new URL(config.proxy_url?.trim() ?? "");
      if (!["http:", "https:"].includes(u.protocol) || !u.hostname || u.username || u.password || u.search || u.hash) return false;
    } catch { return false; }
  }
  return officialEndpoint(config.endpoint, provider) && s.llm_profiles.some(p => p.id === config.profile_id && p.api_key_configured && officialEndpoint(p.config.endpoint, provider))
    && (!("proxy_enabled" in config) || !config.proxy_enabled || !!config.proxy_url?.trim());
}
export function recognitionChoices(s: AppSettings, models: ModelInfo[], kind: "asr" | "ocr") {
  const local = models.filter(m => m.kind === kind && ["available", "active"].includes(m.status)).map(m => ({ value: kind === "ocr" ? "local" : `local:${m.id}`, label: m.display_name }));
  const cloud = (kind === "asr" ? ["openai", "deepseek"] as const : ["deepseek"] as const).filter(p => cloudReady(s, kind, p)).map(p => ({ value: kind === "asr" ? `cloud:${p}` : p, label: `${p === "openai" ? "OpenAI" : "DeepSeek"} · 云端 ${kind.toUpperCase()}` }));
  return [...(kind === "asr" ? [{ value: "windows", label: "Windows Live Captions · 系统字幕" }] : []), ...local, ...cloud];
}
export function sourceValue(source: CaptionSourceConfig) {
  return source.type === "windows_live_caption" ? "windows" : source.type === "local_asr" ? `local:${source.model_id}` : `cloud:${source.provider}`;
}
export function captionSource(value: string, s: AppSettings): CaptionSourceConfig {
  if (value === "windows") return { type: "windows_live_caption" };
  if (value.startsWith("cloud:")) return { type: "cloud_asr", provider: value.slice(6) as "openai" | "deepseek" };
  return { ...(s.captions.source.type === "local_asr" ? s.captions.source : { type: "local_asr", device: "cuda", compute_type: "int8_float16", vad_profile: s.captions.audio_mode, channel_mode: "mix", channel_switch_sensitivity: "standard", suppress_non_speech_segments: false }), model_id: value.slice(6) } as CaptionSourceConfig;
}

export function translationProfile(s: AppSettings, mode: TranslationMode) {
  return s.llm_profiles.find(p => p.id === translationRoutes(s)[mode])?.config;
}
