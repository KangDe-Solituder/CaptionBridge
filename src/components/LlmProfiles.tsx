import { BrainCircuit, Check, Loader2, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { activateLlmProfile, removeLlmProfile, saveLlmProfile } from "../lib/api";
import type { LlmProfile, LlmProviderSettings, SettingsView } from "../lib/contracts";
import { previewSettings } from "../lib/ui-shared";
import { isTauri } from "../lib/window";

export function LlmProfiles({ view, onChanged, onMessage, onTest, testing }: {
  view: SettingsView; onChanged: (view: SettingsView) => void; onMessage: (message: string) => void;
  onTest: () => void; testing: boolean;
}) {
  const [editor, setEditor] = useState<LlmProfile | "new" | null>(null);
  const [busy, setBusy] = useState(false);
  const profiles = view.settings.llm_profiles;
  const active = view.settings.llm.profile_id;
  const preview = (next: LlmProfile[], selected: string | null | undefined) => ({
    ...view, api_key_configured: next.some(p => p.id === selected && p.api_key_configured),
    settings: { ...view.settings, llm_profiles: next, llm: next.find(p => p.id === selected)?.config ?? { ...previewSettings.llm } },
  });
  const act = async (run: () => Promise<SettingsView>, message: string) => {
    setBusy(true);
    try { onChanged(await run()); onMessage(message); } catch (error) { onMessage(String(error)); }
    finally { setBusy(false); }
  };
  return <div className="llm-profiles">
    <div className="profile-toolbar"><p>{profiles.length ? "选择一套配置，用于后续的划词、字幕和截图翻译。" : "连接你的语言模型，开始翻译。"}</p><button className="primary" disabled={busy} onClick={() => setEditor("new")}><Plus />添加配置</button></div>
    {profiles.length ? <div className="profile-grid">{profiles.map(profile => <article className="profile-card" data-active={active === profile.id} key={profile.id}>
      <button className="profile-select" aria-pressed={active === profile.id} disabled={busy} onClick={() => void act(() => isTauri ? activateLlmProfile(profile.id) : Promise.resolve(preview(profiles, profile.id)), "已切换翻译配置；新任务将使用此配置")}>
        <span className="profile-status">{active === profile.id ? <><Check />正在使用</> : "点击启用"}</span>
        <h3>{profile.name}</h3><p className="profile-model">{profile.config.model}</p><p className="profile-endpoint">{profile.config.endpoint}</p>
        <small>Chat Completions · {profile.api_key_configured ? "密钥已托管" : "无 API Key"}</small>
      </button>
      <div className="profile-actions"><button disabled={busy} onClick={() => setEditor(profile)}>编辑</button><button disabled={busy} onClick={() => {
        if (confirm(`移除“${profile.name}”及其保存的密钥？`)) void act(() => isTauri ? removeLlmProfile(profile.id) : Promise.resolve(preview(profiles.filter(p => p.id !== profile.id), active === profile.id ? null : active)), "配置已移除");
      }}>移除</button></div>
    </article>)}</div> : <div className="profile-empty"><BrainCircuit /><h3>还没有翻译配置</h3><p>添加服务地址和模型名称，即可连接云端或本地模型。</p><button className="ghost" onClick={() => setEditor("new")}><Plus />添加第一套配置</button></div>}
    {profiles.length > 0 && <div className="profile-test"><button className="ghost" disabled={!active || busy || testing} onClick={onTest}>{testing ? <Loader2 className="spin" /> : <BrainCircuit />}测试当前配置</button>{!active && <span>请选择一套配置以启用翻译。</span>}</div>}
    <p className="profile-note">配置独立保存。API Key 仅存放在 Windows 凭据管理器中，编辑时不会回填。测试会向当前模型发送一句示例文本。</p>
    {editor && <ProfileEditor profile={editor === "new" ? undefined : editor} onClose={() => setEditor(null)} onSave={async (input) => {
      let updated: SettingsView;
      if (isTauri) updated = await saveLlmProfile(input);
      else {
        const id = input.id ?? crypto.randomUUID();
        const previous = profiles.find(p => p.id === id);
        if (previous?.api_key_configured && previous.config.endpoint !== input.config.endpoint && !input.api_key?.trim() && !input.clear_api_key) throw new Error("接口地址已更改，请重新填写或清除密钥");
        const next = { id, name: input.name, config: { ...input.config, profile_id: id }, api_key_configured: !input.clear_api_key && (!!input.api_key?.trim() || !!previous?.api_key_configured) };
        updated = preview(previous ? profiles.map(p => p.id === id ? next : p) : [...profiles, next], profiles.length === 0 ? id : active);
      }
      onChanged(updated); onMessage("翻译配置已保存");
    }} />}
  </div>;
}

function ProfileEditor({ profile, onClose, onSave }: { profile?: LlmProfile; onClose: () => void; onSave: (input: { id?: string; name: string; config: LlmProviderSettings; api_key?: string; clear_api_key: boolean }) => Promise<void> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(profile?.name ?? "");
  const [config, setConfig] = useState<LlmProviderSettings>(() => profile ? { ...profile.config } : { ...previewSettings.llm, endpoint: "", model: "" });
  const [key, setKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const patch = (next: Partial<LlmProviderSettings>) => setConfig(value => ({ ...value, ...next }));
  useEffect(() => { const node = dialog.current; node?.showModal(); return () => node?.close(); }, []);
  return createPortal(<dialog className="profile-dialog" ref={dialog} aria-labelledby="profile-dialog-title" onCancel={event => { event.preventDefault(); if (!busy) { setKey(""); onClose(); } }}>
    <form onSubmit={async event => {
      event.preventDefault(); setBusy(true); setError("");
      try { await onSave({ id: profile?.id, name: name.trim(), config, api_key: key || undefined, clear_api_key: clearKey }); setKey(""); onClose(); }
      catch (e) { setError(String(e)); } finally { setBusy(false); }
    }}>
      <header><div><span className="eyebrow">TRANSLATION SERVICE</span><h2 id="profile-dialog-title">{profile ? "编辑配置" : "添加翻译配置"}</h2><p>连接兼容 OpenAI Chat Completions 的服务。</p></div><button type="button" className="ghost" aria-label="关闭配置编辑" disabled={busy} onClick={() => { setKey(""); onClose(); }}><X /></button></header>
      <fieldset disabled={busy}>
        <label className="profile-field">配置名称<input autoFocus required maxLength={80} placeholder="例如：日常翻译、本地模型" value={name} onChange={e => setName(e.target.value)} /></label>
        <label className="profile-field">接口地址<input type="url" required placeholder="https://api.example.com/v1" autoComplete="off" value={config.endpoint} onChange={e => patch({ endpoint: e.target.value })} /></label>
        <div className="two-cols"><label className="profile-field">模型名称<input required placeholder="服务提供的模型 ID" value={config.model} onChange={e => patch({ model: e.target.value })} /></label><label className="profile-field">目标语言<input required value={config.target_language} onChange={e => patch({ target_language: e.target.value })} /></label></div>
        <label className="profile-field">API Key <small>{profile?.api_key_configured ? "已安全保存 · 留空保留原密钥" : "可选 · 无需认证的本地服务可留空"}</small><input type="password" autoComplete="new-password" value={key} disabled={clearKey} placeholder={profile?.api_key_configured ? "仅在更换密钥时填写" : "输入 API Key"} onChange={e => setKey(e.target.value)} /></label>
        {profile?.api_key_configured && <label className="profile-clear"><input type="checkbox" checked={clearKey} onChange={e => { setClearKey(e.target.checked); if (e.target.checked) setKey(""); }} />清除这套配置已保存的密钥</label>}
        <details className="settings-advanced"><summary>高级请求设置<span>思考模式、响应时限与附加参数</span></summary>
          <label className="profile-clear"><input type="checkbox" checked={config.thinking_enabled} onChange={e => patch({ thinking_enabled: e.target.checked })} />启用思考模式</label>
          <div className="two-cols"><label className="profile-field">字幕首 token 截止（秒）<input type="number" min={1} max={5} step={.25} value={config.timeout_milliseconds / 1000} onChange={e => patch({ timeout_milliseconds: Number(e.target.value) * 1000 })} /></label><label className="profile-field">最大输出 tokens<input type="number" min={128} max={32768} value={config.max_tokens} onChange={e => patch({ max_tokens: Number(e.target.value) })} /></label></div>
          <label className="profile-field">Extra Body JSON<textarea placeholder="{}" value={config.extra_body_json} onChange={e => patch({ extra_body_json: e.target.value })} /></label>
        </details>
      </fieldset>
      {error && <p className="profile-error" role="alert">{error}</p>}
      <footer><span>保存后密钥输入框会清空。</span><button className="ghost" type="button" disabled={busy} onClick={() => { setKey(""); onClose(); }}>取消</button><button className="primary" disabled={busy}>{busy ? <Loader2 className="spin" /> : <Check />}保存配置</button></footer>
    </form>
  </dialog>, document.body);
}
