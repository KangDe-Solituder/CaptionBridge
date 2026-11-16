import type { AppSettings } from "./contracts";

// Track saved bindings, including sources that are not currently selected.
export function profileDependents(settings: AppSettings, id: string): { api: string[]; model: string[] } {
  const api: string[] = [];
  const model: string[] = [];
  if (settings.captions.cloud.openai.profile_id === id) api.push("OpenAI ASR");
  if (settings.captions.cloud.deepseek.profile_id === id) api.push("DeepSeek ASR");
  if (settings.ocr.cloud_profile_id === id) api.push("DeepSeek OCR");
  if (settings.translation_profiles?.selection === id) model.push("划词翻译");
  if (settings.translation_profiles?.live_caption === id) model.push("字幕翻译");
  if (settings.translation_profiles?.screenshot === id) model.push("截图翻译");
  return { api, model };
}
