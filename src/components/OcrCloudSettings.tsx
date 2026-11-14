import { Select } from "./Select";
import { HelpTip } from "./HelpTip";
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
      <div className="model-copy"><div><strong><Cloud size={16} /> DeepSeek OCR · 云端</strong><HelpTip text="仅上传框选图片进行文字识别；后续翻译使用独立选择的翻译服务。测试发送内置示例图片，会产生少量 OCR 费用。" /><span className="badge">{configured ? "已配置" : "待配置"}</span></div>
        <p>无需下载模型 · 文字识别 · {profile ? `关联：${profile.name}` : "选择已有的 DeepSeek 官方配置"}</p>

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
  useEffect(() => { const node = dialog.current; node?.showModal(); node?.querySelector<HTMLButtonElement>('[role="combobox"]')?.focus(); return () => node?.close(); }, []);
  return createPortal(<dialog ref={dialog} className="profile-dialog cloud-config-dialog" aria-labelledby="ocr-cloud-title" onCancel={e => { e.preventDefault(); onClose(); }}>
    <form onSubmit={e => { e.preventDefault(); if (selected && valid) onApply({ ...config, cloud_profile_id: id, cloud_endpoint: endpoint.trim() }); }}>
      <header><div><span className="eyebrow">CLOUD OCR</span><h2 id="ocr-cloud-title">DeepSeek 文字识别<HelpTip text="复用已保存的凭据，无需再次填写密钥。" /></h2></div><button type="button" className="ghost" aria-label="关闭 OCR 编辑" onClick={onClose}><X /></button></header>
      <fieldset>
        <div className="cloud-config-section cloud-config-line">
          <label className="profile-field"><span>关联凭据配置<HelpTip text="仅列出使用官方 HTTPS 接口且已保存密钥的配置；不会改变当前翻译来源。" /></span>
            <Select autoFocus aria-label="关联凭据配置" value={selected ? id : ""} onChange={setId}
              options={[{ value: "", label: "请选择 DeepSeek 官方配置" }, ...profiles.map(p => ({ value: p.id, label: p.name + " · " + p.config.model }))]} />
          </label>
          <button type="button" className="ghost" onClick={onManageProfiles}>管理翻译配置</button>
        </div>
        <div className="cloud-config-section">
          <div className="cloud-config-line">
            <label className="profile-field"><span>OCR API 地址<HelpTip text="固定使用文字识别模式。密钥只发送到官方域名，不跟随重定向。" /></span><input required type="url" disabled={!selected} value={endpoint} onChange={e => setEndpoint(e.target.value)} autoComplete="off" /></label>
            <button type="button" className="ghost" disabled={!selected} onClick={() => setEndpoint("https://api.deepseek.com/v1/tools/ocr")}>恢复官方地址</button>
          </div>
          {selected && !valid && <p className="profile-error" role="alert">请填写 api.deepseek.com 的 HTTPS 地址，不支持自定义端口、查询参数或片段。</p>}
        </div>
      </fieldset>
      <footer className="cloud-config-footer">
        {config.cloud_profile_id && <div className="profile-unlink"><button type="button" className="ghost" disabled={config.provider === "deepseek"} onClick={() => onApply({ ...config, cloud_profile_id: null })}>解除关联</button><HelpTip text="如果仍为当前来源，请先选择本地 OCR 并保存。解除后请保存设置，再删除翻译配置。" /></div>}
        <div className="cloud-config-confirm"><button type="button" className="ghost" onClick={onClose}>取消</button><button className="primary" disabled={!selected || !valid}><Check />应用</button></div>
        <p className="cloud-config-note">应用后保存设置生效</p>
      </footer>
    </form>
  </dialog>, document.body);
}
