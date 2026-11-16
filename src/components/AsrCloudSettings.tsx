import { cloudReady } from "../lib/model-choices";
import { invoke } from "@tauri-apps/api/core";
import { Check, Cloud, Loader2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { AppSettings, CloudAsrConfig } from "../lib/contracts";
import { isTauri } from "../lib/window";
import { HelpTip } from "./HelpTip";
import { Select } from "./Select";
import { Toggle } from "./Toggle";

type Provider = "openai" | "deepseek";
const defaults = { openai: "https://api.openai.com/v1/audio/transcriptions", deepseek: "https://api.deepseek.com/v1/tools/asr" };
export function officialAsrEndpoint(value: string, provider: Provider) {
  try { const u = new URL(value); return u.protocol === "https:" && u.hostname === `api.${provider}.com` && (!u.port || u.port === "443") && !u.username && !u.password && !u.search && !u.hash; } catch { return false; }
}
export function AsrCloudSettings({ settings, onChange, onMessage, onManageProfiles }: {
  settings: AppSettings; onChange: (value: AppSettings["captions"]) => void; onMessage: (text: string) => void; onManageProfiles: () => void;
}) {
  const [editing, setEditing] = useState<Provider>();
  const [testing, setTesting] = useState<Provider>();
  const source = settings.captions.source;
  return <div className="cloud-asr-list">{(["openai", "deepseek"] as const).map(provider => {
    const config = settings.captions.cloud[provider];
    const profile = settings.llm_profiles.find(p => p.id === config.profile_id && p.api_key_configured && officialAsrEndpoint(p.config.endpoint, provider));
  const configured = cloudReady(settings, "asr", provider);
    return <article className="model-row" key={provider}><div className="model-copy"><div><strong><Cloud size={16} /> {provider === "openai" ? "OpenAI · GPT-Transcribe" : "DeepSeek ASR"}</strong><HelpTip text="上传系统音频进行识别，最长 6 秒，遇停顿提前提交。识别费用由关联账户承担；后续翻译使用独立选择的翻译服务。当前针对日语配信配置。" /><span className="badge">{configured ? "已配置" : "待配置"}</span></div><p>{profile ? `关联：${profile.name}` : "尚未关联凭据"} · {config.proxy_enabled ? "独立代理" : "直连"}</p></div>
      <div className="model-actions">
      <button className="ghost" onClick={() => setEditing(provider)}>编辑</button><button className="ghost" disabled={!configured || !!testing} onClick={async () => { setTesting(provider); try { onMessage(isTauri ? await invoke<string>("test_cloud_asr", { provider, config }) : "请在原生程序中测试"); } catch (e) { onMessage(String(e)); } finally { setTesting(undefined); } }}>{testing === provider && <Loader2 className="spin" />}测试</button></div></article>;
  })}{editing && <Editor provider={editing} config={settings.captions.cloud[editing]} selected={source.type === "cloud_asr" && source.provider === editing} settings={settings} onClose={() => setEditing(undefined)} onManage={() => { setEditing(undefined); onManageProfiles(); }} onApply={config => { onChange({ ...settings.captions, cloud: { ...settings.captions.cloud, [editing]: config } }); setEditing(undefined); onMessage("ASR 配置已更新，请保存设置"); }} />}</div>;
}

function Editor({ provider, config, settings, selected, onClose, onManage, onApply }: { provider: Provider; config: CloudAsrConfig; settings: AppSettings; selected: boolean; onClose: () => void; onManage: () => void; onApply: (c: CloudAsrConfig) => void }) {
  const dialog = useRef<HTMLDialogElement>(null); const [draft, setDraft] = useState({ ...config });
  const profiles = settings.llm_profiles.filter(p => p.api_key_configured && officialAsrEndpoint(p.config.endpoint, provider));
  const linked = profiles.some(p => p.id === draft.profile_id);
  let proxyValid = !draft.proxy_enabled;
  try { const u = new URL(draft.proxy_url); proxyValid ||= ["http:", "https:"].includes(u.protocol) && !u.username && !u.password && !u.search && !u.hash; } catch { /* Empty proxy is invalid only when enabled. */ }
  const valid = linked && officialAsrEndpoint(draft.endpoint, provider) && proxyValid;
  useEffect(() => { const d = dialog.current; d?.showModal(); d?.querySelector<HTMLButtonElement>('[role="combobox"]')?.focus(); return () => d?.close(); }, []);
  return createPortal(<dialog ref={dialog} className="profile-dialog cloud-config-dialog" aria-labelledby="cloud-asr-title" onCancel={e => { e.preventDefault(); onClose(); }}>
    <form onSubmit={e => { e.preventDefault(); if (valid) onApply(draft); }}>
      <header><div><span className="eyebrow">CLOUD ASR</span><h2 id="cloud-asr-title">{provider === "openai" ? "OpenAI" : "DeepSeek"} 语音识别<HelpTip text="借用已托管的 API 密钥，不改变当前翻译服务。测试会上传 1 秒内置静音音频，产生少量识别费用。" /></h2></div><button type="button" className="ghost" onClick={onClose} aria-label="关闭"><X /></button></header>
      <fieldset>
        <div className="cloud-config-section cloud-config-line">
          <label className="profile-field"><span>关联凭据配置<HelpTip text="仅列出同一服务商的官方 HTTPS 配置，并且必须已保存密钥。删除或清空被关联配置前，需要先解除关联。" /></span>
            <Select autoFocus aria-label="关联凭据配置" value={linked ? draft.profile_id ?? "" : ""} onChange={profile_id => setDraft({ ...draft, profile_id })}
              options={[{ value: "", label: "请选择已有配置" }, ...profiles.map(p => ({ value: p.id, label: p.name + " · " + p.config.model }))]} />
          </label>
          <button className="ghost" type="button" onClick={onManage}>管理翻译配置</button>
        </div>
        <div className="cloud-config-section cloud-config-line">
          <label className="profile-field"><span>ASR API 地址<HelpTip text="密钥只会发送到对应服务商的官方 HTTPS 域名；不跟随重定向。" /></span><input type="url" required disabled={!linked} value={draft.endpoint} onChange={e => setDraft({ ...draft, endpoint: e.target.value })} /></label>
          <button className="ghost" type="button" disabled={!linked} onClick={() => setDraft({ ...draft, endpoint: defaults[provider] })}>恢复官方地址</button>
        </div>
        <div className="cloud-config-section">
          <div className="setting-row"><h3>独立代理<HelpTip text="开启后，此 ASR 来源只通过该代理连接；失败不会退回直连。与 LLM、OCR 及下载代理分别配置。" /></h3><Toggle label="启用 ASR 代理" checked={draft.proxy_enabled} onChange={proxy_enabled => setDraft({ ...draft, proxy_enabled })} /></div>
          {draft.proxy_enabled && <label className="profile-field cloud-proxy-field">代理地址<input type="url" required placeholder="http://127.0.0.1:10808" value={draft.proxy_url} onChange={e => setDraft({ ...draft, proxy_url: e.target.value })} /></label>}
        </div>
        {!valid && <p role="status" className="profile-error">请关联有效配置，并检查接口与代理地址。</p>}
      </fieldset>
      <footer className="cloud-config-footer">
        {config.profile_id && <div className="profile-unlink"><button type="button" className="ghost" disabled={selected} onClick={() => onApply({ ...draft, profile_id: null })}>解除关联</button><HelpTip text={selected ? "请先在功能与入口切换识别来源并保存，再解除关联。" : "解除后该识别来源将不可用，不会删除翻译配置或密钥。应用后需要保存设置。"} /></div>}
        <div className="cloud-config-confirm"><button type="button" className="ghost" onClick={onClose}>取消</button><button className="primary" disabled={!valid}>应用</button></div>
        <p className="cloud-config-note">应用后保存设置生效</p>
      </footer>
    </form>
  </dialog>, document.body);
}
