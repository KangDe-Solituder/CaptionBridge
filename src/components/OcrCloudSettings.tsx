import { invoke } from "@tauri-apps/api/core";
import { Check, Cloud, Loader2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { AppSettings, OcrSettings } from "../lib/contracts";
import { isTauri } from "../lib/window";

export function officialDeepSeekEndpoint(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "api.deepseek.com" && (!url.port || url.port === "443")
      && !url.username && !url.password && !url.search && !url.hash;
  } catch { return false; }
}

export function OcrCloudSettings({ settings, onChange, onMessage, onManageProfiles }: {
  settings: AppSettings; onChange: (value: OcrSettings) => void; onMessage: (text: string) => void; onManageProfiles: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [testing, setTesting] = useState(false);
  const config = settings.ocr;
  const profiles = settings.llm_profiles.filter(p => officialDeepSeekEndpoint(p.config.endpoint) && p.api_key_configured);
  const profile = profiles.find(p => p.id === config.cloud_profile_id);
  const configured = !!profile && officialDeepSeekEndpoint(config.cloud_endpoint);
  return <div className="settings-group">
    <article className="model-row">
      <div className="model-copy"><div><strong><Cloud size={16} /> DeepSeek OCR · 云端</strong><span className="badge">{configured ? "已配置" : "待配置"}</span></div>
        <p>无需下载模型 · 文字识别 · {profile ? `关联：${profile.name}` : "选择已有的 DeepSeek 官方配置"}</p>
        <small>仅上传框选图片；识别后的翻译仍使用当前翻译服务。配置修改后点击「保存设置」生效。</small>
      </div>
      <div className="model-actions">
        {config.provider === "deepseek" ? <span className="model-selected"><Check />当前来源</span> : <button className="primary" disabled={!configured || testing} onClick={() => { onChange({ ...config, provider: "deepseek" }); onMessage("已选择 DeepSeek OCR，保存设置后生效"); }}>选中</button>}
        <button className="ghost" disabled={testing} onClick={() => setEditing(true)}>编辑</button>
        <button className="ghost" disabled={!configured || testing} onClick={async () => {
          setTesting(true);
          try { onMessage(isTauri ? await invoke<string>("test_cloud_ocr", { config }) : "浏览器预览不能调用 OCR，请在桌面应用中测试"); }
          catch (error) { onMessage(String(error)); } finally { setTesting(false); }
        }}>{testing && <Loader2 className="spin" />}测试</button>
      </div>
    </article>
    {editing && <OcrEditor config={config} profiles={profiles} onClose={() => setEditing(false)} onManageProfiles={() => { setEditing(false); onManageProfiles(); }} onApply={value => {
      onChange(value); setEditing(false); onMessage("OCR 配置已更新，点击保存设置后生效");
    }} />}
  </div>;
}

function OcrEditor({ config, profiles, onClose, onApply, onManageProfiles }: {
  config: OcrSettings; profiles: AppSettings["llm_profiles"]; onClose: () => void;
  onApply: (value: OcrSettings) => void; onManageProfiles: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [id, setId] = useState(config.cloud_profile_id ?? "");
  const [endpoint, setEndpoint] = useState(config.cloud_endpoint);
  const selected = profiles.some(p => p.id === id);
  const valid = officialDeepSeekEndpoint(endpoint.trim());
  useEffect(() => { const node = dialog.current; node?.showModal(); return () => node?.close(); }, []);
  return createPortal(<dialog ref={dialog} className="profile-dialog" aria-labelledby="ocr-cloud-title" onCancel={e => { e.preventDefault(); onClose(); }}>
    <form onSubmit={e => { e.preventDefault(); if (selected && valid) onApply({ ...config, cloud_profile_id: id, cloud_endpoint: endpoint.trim() }); }}>
      <header><div><span className="eyebrow">CLOUD OCR</span><h2 id="ocr-cloud-title">DeepSeek 文字识别</h2><p>复用已保存的凭据，无需再次填写密钥。</p></div><button type="button" className="ghost" aria-label="关闭 OCR 编辑" onClick={onClose}><X /></button></header>
      <fieldset>
        <label className="profile-field">关联翻译配置<select required value={selected ? id : ""} onChange={e => setId(e.target.value)}><option value="">请选择 DeepSeek 官方配置</option>{profiles.map(p => <option key={p.id} value={p.id}>{p.name} · {p.config.model}</option>)}</select></label>
        <p>仅列出使用官方 HTTPS 接口且已保存密钥的配置；不会改变当前翻译来源。</p>
        <button type="button" className="ghost" onClick={onManageProfiles}>管理翻译配置</button>
        <label className="profile-field">OCR API 地址<input required type="url" disabled={!selected} value={endpoint} onChange={e => setEndpoint(e.target.value)} autoComplete="off" /></label>
        {selected && !valid && <p className="profile-error" role="alert">请填写 api.deepseek.com 的 HTTPS 地址，不支持自定义端口、查询参数或片段。</p>}
        <button type="button" className="ghost" disabled={!selected} onClick={() => setEndpoint("https://api.deepseek.com/v1/tools/ocr")}>恢复官方默认地址</button>
        <p>固定使用文字识别模式。测试会发送一张内置示例图片，并产生少量 OCR 费用。</p>
      </fieldset>
      <footer><span>应用后，点击主页面「保存设置」。</span><button type="button" className="ghost" onClick={onClose}>取消</button><button className="primary" disabled={!selected || !valid}><Check />应用</button></footer>
    </form>
  </dialog>, document.body);
}
