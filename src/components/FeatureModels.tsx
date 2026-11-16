import type { AppSettings, ModelInfo, TranslationMode } from "../lib/contracts";
import { captionSource, recognitionChoices, sourceValue, translationRoutes } from "../lib/model-choices";
import { Select } from "./Select";

export function FeatureModels({ mode, settings: s, models, onChange }: { mode: TranslationMode; settings: AppSettings; models: ModelInfo[]; onChange: (s: AppSettings) => void }) {
  const kind = mode === "live_caption" ? "asr" : "ocr";
  const choices = recognitionChoices(s, models, kind);
  const value = kind === "asr" ? sourceValue(s.captions.source) : s.ocr.provider;
  const valid = choices.some(c => c.value === value);
  const routes = translationRoutes(s);
  return <div className="feature-models">
    {mode !== "selection" && <div className="setting-row"><div><strong>识别来源</strong>{!valid && <p>原配置不可用，请下载模型或修复关联凭据后重新选择。</p>}</div>
      <Select aria-label={`${mode} 识别来源`} value={valid ? value : ""} options={[...(!valid ? [{ value: "", label: "原配置不可用 · 请选择", disabled: true }] : []), ...choices]} onChange={next => onChange(kind === "asr" ? { ...s, captions: { ...s.captions, source: captionSource(next, s) } } : { ...s, ocr: { ...s.ocr, provider: next as "local" | "deepseek" } })} />
    </div>}
    <div className="setting-row"><div><strong>翻译模型</strong><p>仅用于{mode === "selection" ? "划词" : mode === "live_caption" ? "字幕" : "截图"}翻译。保存后应用{mode === "live_caption" ? "；运行中的字幕会重新启动" : "于新任务"}。</p></div>
      <Select aria-label={`${mode} 翻译模型`} value={routes[mode] ?? ""} disabled={!s.llm_profiles.length} options={s.llm_profiles.length ? s.llm_profiles.map(p => ({ value: p.id, label: `${p.name} · ${p.config.model}` })) : [{ value: "", label: "暂无配置 · 请先添加翻译服务" }]} onChange={id => onChange({ ...s, translation_profiles: { ...routes, [mode]: id } })} />
    </div>
  </div>;
}
