import type { AppSettings } from "./contracts";

// Track saved bindings, including sources that are not currently selected.
export function profileDependents(settings: AppSettings, id: string): string[] {
  const names: string[] = [];
  if (settings.captions.cloud.openai.profile_id === id) names.push("OpenAI ASR");
  if (settings.captions.cloud.deepseek.profile_id === id) names.push("DeepSeek ASR");
  if (settings.ocr.cloud_profile_id === id) names.push("DeepSeek OCR");
  return names;
}
