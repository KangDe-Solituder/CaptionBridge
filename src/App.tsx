import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Activity, ArrowRight, BrainCircuit, Captions, Check, ChevronDown, ChevronUp, Clipboard, Copy, Download, Eraser,
  ExternalLink, FileText, GripHorizontal, Languages, Loader2, Minus, MousePointer2, Palette, Play,
  RefreshCw, RotateCcw, Search, Settings, Sparkles, Square, Trash2, X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Segmented } from "./components/Segmented";
import { Toggle } from "./components/Toggle";
import {
  clearRuntimeLogs, deleteApiKey, exportCaptionSession, getRuntimeLogs,
  getRuntimeStatus, getSettings, openLogsDir, refreshCaption, saveOverlayPosition, saveSettings,
  setCaptionRunning, testLlm, translateSelection, listModels, downloadModel, cancelModelDownload,
  verifyModel, testModel, deleteModel, openModelsDir, switchCaptionSource,
} from "./lib/api";
import type {
  AppSettings, CaptionRuntimeState, CaptionSourceConfig, CaptionSourceHealth, CaptionTranslatedEvent,
  ModelInfo, ModelProgressEvent, RuntimeLogEntry, RuntimeStatus, SelectionReadyEvent, SettingsView, TranslationResult,
} from "./lib/contracts";

const currentWindow = (("__TAURI_INTERNALS__" in window)
  ? getCurrentWebviewWindow()
  : { label: "main" }) as ReturnType<typeof getCurrentWebviewWindow>;
const windowLabel = currentWindow.label;
document.documentElement.dataset.window = windowLabel;
const bootTheme = localStorage.getItem("livecaption-theme") ?? "system";
document.documentElement.dataset.theme = bootTheme === "system"
  ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
  : bootTheme;

const emptyResult: TranslationResult = { source_text: "", translated_text: "", model: "", latency_ms: 0, first_token_ms: 0, cached: false };
const previewSettings: AppSettings = {
  schema_version: 6,
  llm: { endpoint: "https://api.openai.com/v1", model: "gpt-4o-mini", target_language: "中文", extra_body_json: "", timeout_milliseconds: 5000, max_tokens: 768, temperature: 0.2, thinking_enabled: false },
  selection: { enabled: true, clipboard_fallback_enabled: true, hotkey: "Alt+KeyQ", trigger_mode: "automatic" },
  captions: { source: { type: "local_asr", model_id: "kotoba-whisper-v2.0-faster", device: "cuda", compute_type: "int8_float16", vad_profile: "normal" }, enabled: true, auto_launch: true, poll_milliseconds: 160, stable_milliseconds: 420, max_duration_milliseconds: 1800, max_chars: 96, context_segments: 2, audio_mode: "normal", model_mirror_url: "https://hf-mirror.com" },
  overlay: { opacity: .92, font_size: 22, width: 760, transparent: false, caption_color: "#ffffff", drag_mode: "alt" },
  visual: { theme: "system", blur_enabled: true, blur_scope: "floating", reduce_motion: false },
};

function useTheme(settings?: AppSettings) {
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

export function App() {
  if (windowLabel === "overlay") return <OverlayWindow />;
  if (windowLabel === "selection-toolbar") return <ToolbarWindow />;
  return <MainWindow />;
}

type Page = "selection" | "captions" | "settings";
type SettingsTab = "selection" | "captions" | "llm" | "appearance" | "logs";

function MainWindow() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [bootError, setBootError] = useState("");
  const [page, setPage] = useState<Page>("selection");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("captions");
  const [apiKey, setApiKey] = useState("");
  const [status, setStatus] = useState<RuntimeStatus>({ caption_running: false, caption_state: "stopped", selection_hotkey_registered: false, source_health: "unknown", last_caption_update_ms: 0, reader_restarts: 0, source_error_count: 0, translation_queue_depth: 0, last_translation_latency_ms: 0, last_translation_first_token_ms: 0, selected_source: previewSettings.captions.source, asr_latency_ms: 0 });
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [message, setMessage] = useState("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [selectionResult, setSelectionResult] = useState<TranslationResult>(emptyResult);
  const [captionResults, setCaptionResults] = useState<CaptionTranslatedEvent[]>([]);
  const [logs, setLogs] = useState<RuntimeLogEntry[]>([]);
  const [logQuery, setLogQuery] = useState("");
  const [exportFormat, setExportFormat] = useState("srt");

  const loadInitialSettings = useCallback(async () => {
    setBootError("");
    const watchdog = window.setTimeout(() => {
      setBootError("初始化等待时间过长。后台仍在运行，你可以重试加载界面。");
    }, 4_000);
    try {
      setView(await getSettings());
    } catch (error) {
      if (!("__TAURI_INTERNALS__" in window)) {
        setView({ settings: previewSettings, api_key_configured: false });
      } else {
        setBootError(`读取配置失败：${String(error)}`);
      }
    } finally {
      clearTimeout(watchdog);
    }
  }, []);

  useEffect(() => {
    if ("__TAURI_INTERNALS__" in window) {
      requestAnimationFrame(() => {
        void currentWindow.show();
        void currentWindow.setFocus();
      });
    }
  }, []);

  useEffect(() => {
    void loadInitialSettings();
    void getRuntimeStatus().then(setStatus).catch(() => undefined);
    const unsubs = [
      listen<SettingsView>("settings:changed", e => setView(e.payload)),
      listen<TranslationResult>("translation:finished", e => setSelectionResult(e.payload)),
      listen<CaptionTranslatedEvent>("caption:translated", e => setCaptionResults(items => [...items, e.payload].slice(-30))),
      listen<CaptionRuntimeState>("caption:state-changed", e => setStatus(s => ({ ...s, caption_state: e.payload, caption_running: ["running", "starting", "loading_model", "switching"].includes(e.payload) }))),
      listen<CaptionSourceHealth>("caption:health-changed", e => setStatus(s => ({ ...s, source_health: e.payload }))),
      listen<ModelProgressEvent>("model:progress", e => {
        const p = e.payload;
        setModels(items => items.map(item => item.id === p.model_id ? { ...item, status: p.status, downloaded_bytes: p.downloaded_bytes, error: p.error } : item));
        if (p.status === "available" || p.status === "corrupt" || p.status === "failed" || p.status === "incompatible") void listModels().then(setModels);
      }),
      listen<void>("caption:session-reset", () => setCaptionResults([])),
      listen<string>("runtime:error", e => setMessage(e.payload)),
    ];
    const interval = window.setInterval(() => void getRuntimeStatus().then(setStatus).catch(() => undefined), 1200);
    void listModels().then(setModels).catch(() => undefined);
    const closeRequested = "__TAURI_INTERNALS__" in window
      ? currentWindow.onCloseRequested(event => { event.preventDefault(); void currentWindow.hide(); })
      : undefined;
    return () => { clearInterval(interval); unsubs.forEach(p => void p.then(u => u())); if (closeRequested) void closeRequested.then(u => u()); };
  }, [loadInitialSettings]);

  useEffect(() => {
    if (settingsTab !== "logs" || page !== "settings") return;
    void getRuntimeLogs().then(setLogs);
    const id = window.setInterval(() => void getRuntimeLogs().then(setLogs), 2000);
    return () => clearInterval(id);
  }, [page, settingsTab]);

  useTheme(view?.settings);
  const settings = view?.settings;
  const update = useCallback((next: AppSettings) => setView(v => v ? { ...v, settings: next } : v), []);

  const persist = async () => {
    if (!settings) return;
    setSaving(true); setMessage("");
    try {
      const saved = await saveSettings(settings, apiKey || undefined);
      setView(saved); setApiKey(""); setMessage("设置已保存");
    } catch (error) { setMessage(String(error)); }
    finally { setSaving(false); }
  };

  const runTest = async () => {
    setTesting(true); setMessage("");
    try { const result = await testLlm(apiKey || undefined); setSelectionResult(result); setMessage("LLM 连接正常"); }
    catch (error) { setMessage(String(error)); }
    finally { setTesting(false); }
  };

  const toggleCaptions = async () => {
    if (!settings) return;
    const next = !status.caption_running;
    if (next && settings.captions.source.type === "local_asr") {
      const modelId = settings.captions.source.model_id;
      let currentModels: ModelInfo[];
      try {
        currentModels = await listModels();
        setModels(currentModels);
      } catch (error) {
        setMessage(`无法检查本地模型：${String(error)}`);
        return;
      }
      const selected = currentModels.find(model => model.id === modelId);
      if (!selected) {
        setMessage(`模型清单中不存在 ${modelId}，未启动下载`);
        return;
      }
      if (selected.status === "downloading") {
        setMessage(`${selected.display_name} 正在下载中，请等待下载和校验完成`);
        return;
      }
      if (selected.status === "verifying" || selected.status === "loading") {
        setMessage(`${selected.display_name} 正在${selected.status === "verifying" ? "校验" : "测试"}中`);
        return;
      }
      if (selected.status !== "available" && selected.status !== "active") {
        const size = (selected.expected_size_bytes / 1024 / 1024 / 1024).toFixed(2);
        if (window.confirm(`未找到可用的 ${selected.display_name}（${size} GB）。\n\n状态：${modelStatusText(selected.status)}\n保存位置：%LOCALAPPDATA%\\com.dimfi.livecaption\\models\n来源：Hugging Face 官方源（失败后使用镜像）\n\n现在开始下载吗？`)) {
          try { await downloadModel(modelId); setMessage("模型下载已开始，可在设置 → 字幕中查看进度"); }
          catch (error) { setMessage(String(error)); }
        }
        return;
      }
    }
    try { await setCaptionRunning(next); setStatus(s => ({ ...s, caption_running: next, caption_state: next ? "starting" : "stopping" })); }
    catch (error) { setMessage(String(error)); }
  };

  const selectSource = async (source: CaptionSourceConfig) => {
    try {
      await switchCaptionSource(source);
      setView(await getSettings());
      setStatus(await getRuntimeStatus());
      setMessage(status.caption_running ? "正在切换字幕来源" : "字幕来源已更新");
    } catch (error) { setMessage(String(error)); }
  };

  const exportCaptions = async () => {
    const path = await save({ defaultPath: `LiveCaption-${new Date().toISOString().slice(0, 10)}.${exportFormat}`, filters: [{ name: "字幕", extensions: [exportFormat] }] });
    if (!path) return;
    try { await exportCaptionSession(path, exportFormat); setMessage("字幕已导出"); }
    catch (error) { setMessage(String(error)); }
  };

  if (!settings || !view) return (
    <main className="boot">
      {bootError
        ? <section className="boot-card"><h2>界面尚未完成加载</h2><p>{bootError}</p><button className="primary" onClick={() => void loadInitialSettings()}><RefreshCw />重新加载</button></section>
        : <Loader2 className="spin" aria-label="正在加载" />}
    </main>
  );

  return (
    <main className="shell">
      <aside className="rail">
        <Logo />
        <nav>
          <RailButton active={page === "selection"} label="划词翻译" onClick={() => setPage("selection")}><Clipboard /></RailButton>
          <RailButton active={page === "captions"} label="实时字幕" onClick={() => setPage("captions")}><Captions /></RailButton>
          <RailButton active={page === "settings"} label="设置" onClick={() => setPage("settings")}><Settings /></RailButton>
        </nav>
        <span className="rail-spacer" />
        <span className="version">v0.5</span>
      </aside>

      <section className="content">
        {page === "selection" && <SelectionPage settings={settings} status={status} result={selectionResult} />}
        {page === "captions" && <CaptionPage status={status} items={captionResults} exportFormat={exportFormat} setExportFormat={setExportFormat} onToggle={toggleCaptions} onExport={exportCaptions} />}
        {page === "settings" && (
          <SettingsPage tab={settingsTab} setTab={setSettingsTab} view={view} apiKey={apiKey} setApiKey={setApiKey}
            update={update} onSave={persist} saving={saving} testing={testing} onTest={runTest}
            models={models} status={status} onSelectSource={selectSource} refreshModels={() => void listModels().then(setModels)} onMessage={setMessage}
            logs={logs} logQuery={logQuery} setLogQuery={setLogQuery}
            onClearLogs={async () => { await clearRuntimeLogs(); setLogs([]); }} />
        )}
        {message && <button className="toast" onClick={() => setMessage("")}><span>{message}</span><X size={15} /></button>}
      </section>
    </main>
  );
}

function PageHeader({ eyebrow, title, description, action }: { eyebrow: string; title: string; description: string; action?: React.ReactNode }) {
  return <header className="page-header"><div><span>{eyebrow}</span><h1>{title}</h1><p>{description}</p></div>{action}</header>;
}

function SelectionPage({ settings, status, result }: { settings: AppSettings; status: RuntimeStatus; result: TranslationResult }) {
  const automatic = settings.selection.trigger_mode === "automatic";
  return <div className="page">
    <PageHeader eyebrow="QUICK TRANSLATE" title="划词翻译" description={automatic ? "选中文字后，翻译按钮会立即出现在光标旁。" : "选中文字，用快捷键唤出轻巧的翻译按钮。"}
      action={<StatusDot on={status.selection_hotkey_registered} text={status.selection_hotkey_registered ? "服务已就绪" : "服务未启用"} />} />
    <section className="workflow-strip">
      <div className="workflow-icon">{automatic ? <MousePointer2 /> : <span>{displayHotkey(settings.selection.hotkey)}</span>}</div>
      <div><span className="overline">{automatic ? "自动感知选区" : "全局快捷键"}</span><h2>{automatic ? "松开鼠标，即刻出现" : `${displayHotkey(settings.selection.hotkey)} · 随时唤起`}</h2><p>浮动工具只在有效选区旁出现，不抢焦点，不打断当前应用。</p></div>
      <div className="flow-steps"><span>选择</span><ArrowRight /><span>点击翻译</span><ArrowRight /><span>查看结果</span></div>
    </section>
    <section className="recent-section"><div className="section-label"><span>最近翻译</span>{result.translated_text && <small>{result.latency_ms}ms</small>}</div>{result.translated_text ? <ResultBlock result={result} /> : <div className="quiet-empty"><Languages /><div><h3>还没有翻译记录</h3><p>去任意应用选中一段文字试试。</p></div></div>}</section>
  </div>;
}

const stateText: Record<CaptionRuntimeState, string> = { stopped: "已停止", starting: "启动中", running: "运行中", stopping: "停止中", switching: "正在切换字幕来源", loading_model: "正在加载模型", error: "发生错误" };
const healthText: Record<CaptionSourceHealth, string> = {
  unknown: "等待字幕源",
  ready: "正在接收",
  quiet: "暂无新声音",
  status_prompt: "系统提示已暂停",
  stale: "字幕源没有响应",
  reconnecting: "正在自动重连",
  error: "读取异常",
};
function CaptionHealth({ status }: { status: RuntimeStatus }) {
  const age = status.last_caption_update_ms ? Math.max(0, Date.now() - status.last_caption_update_ms) : 0;
  const ageText = status.last_caption_update_ms ? `${Math.round(age / 1000)}s 前更新` : "尚未收到新句子";
  return <section className="caption-health" data-health={status.source_health}>
    <div className="health-main"><span className="health-icon"><Activity /></span><div><strong>{healthText[status.source_health]}</strong><small>{ageText}</small></div></div>
    <div className="health-metrics"><span>{sourceName(status.active_source ?? status.selected_source)}</span><span>ASR {status.asr_latency_ms ? `${status.asr_latency_ms}ms` : "—"}</span><span>队列 {status.translation_queue_depth}/4</span><span>首 token {status.last_translation_first_token_ms ? `${status.last_translation_first_token_ms}ms` : "—"}</span></div>
  </section>;
}
function sourceName(source?: CaptionSourceConfig) {
  if (!source || source.type === "windows_live_caption") return "Windows Live Caption";
  return source.model_id === "kotoba-whisper-v2.0-faster" ? "Kotoba Whisper v2.0" : "Whisper large-v3-turbo";
}
function CaptionPage({ status, items, exportFormat, setExportFormat, onToggle, onExport }: { status: RuntimeStatus; items: CaptionTranslatedEvent[]; exportFormat: string; setExportFormat: (v: string) => void; onToggle: () => void; onExport: () => void }) {
  const busy = status.caption_state === "starting" || status.caption_state === "stopping" || status.caption_state === "switching" || status.caption_state === "loading_model";
  const activeName = sourceName(status.active_source ?? status.selected_source);
  return <div className="page">
    <PageHeader eyebrow="LIVE CAPTIONS" title="实时字幕" description={`当前字幕来源：${activeName}。识别结果按顺序交给 LLM 翻译。`}
      action={<StatusDot on={status.caption_state === "running"} text={stateText[status.caption_state]} />} />
    <section className="caption-launcher" data-running={status.caption_running}>
      <div className="caption-pulse">{busy ? <Loader2 className="spin" /> : <Captions />}</div>
      <div className="caption-copy"><span className="overline">{status.caption_running ? "正在监听" : "准备就绪"}</span><h2>{status.caption_running ? `${activeName} 正在识别` : "让声音变成看得懂的字幕"}</h2><p>{status.caption_running ? "partial 会即时更新原文预览，final 结果才会进入翻译和导出。" : `启动后使用 ${activeName}；本地模型会捕获当前默认输出设备。`}</p></div>
      <button className={status.caption_running ? "danger action" : "primary action"} disabled={busy} onClick={onToggle}>{status.caption_running ? <Square /> : <Play />}{stateText[status.caption_state]}</button>
    </section>
    <CaptionHealth status={status} />
    <div className="section-heading"><div><h2>本次转录</h2><p>{items.length ? `${items.length} 条字幕，最新内容在最上方` : "启动后记录会自动出现在这里"}</p></div><div className="export-row"><select value={exportFormat} onChange={e => setExportFormat(e.target.value)}><option value="srt">SRT</option><option value="vtt">WebVTT</option><option value="txt">TXT</option><option value="json">JSON</option></select><button className="ghost" disabled={!items.length} onClick={onExport}><Download />导出</button></div></div>
    <div className="caption-list">{items.length ? [...items].reverse().map((item, index) => <article key={item.segment.id} data-latest={index === 0}><span>{item.segment.source_text}</span><p>{item.result.translated_text || item.result.error}</p><small>{item.result.latency_ms}ms</small></article>) : <div className="quiet-empty"><FileText /><div><h3>等待第一句字幕</h3><p>这里不会用大面积空框占住你的视线。</p></div></div>}</div>
  </div>;
}

function SettingsPage(props: { tab: SettingsTab; setTab: (v: SettingsTab) => void; view: SettingsView; apiKey: string; setApiKey: (v: string) => void; update: (v: AppSettings) => void; onSave: () => void; saving: boolean; testing: boolean; onTest: () => void; logs: RuntimeLogEntry[]; logQuery: string; setLogQuery: (v: string) => void; onClearLogs: () => void; models: ModelInfo[]; status: RuntimeStatus; onSelectSource: (source: CaptionSourceConfig) => Promise<void>; refreshModels: () => void; onMessage: (message: string) => void }) {
  const { tab, setTab, view, apiKey, setApiKey, update } = props; const s = view.settings;
  const tabs: { id: SettingsTab; label: string }[] = [{ id: "selection", label: "划词" }, { id: "captions", label: "字幕" }, { id: "llm", label: "LLM" }, { id: "appearance", label: "外观" }, { id: "logs", label: "日志" }];
  const filteredLogs = props.logs.filter(l => `${l.level} ${l.message}`.toLowerCase().includes(props.logQuery.toLowerCase()));
  return <div className="page settings-page">
    <PageHeader eyebrow="PREFERENCES" title="设置" description="配置连接、快捷键和视觉体验。" action={tab !== "logs" && <button className="primary" onClick={props.onSave} disabled={props.saving}>{props.saving ? <Loader2 className="spin" /> : <Check />}保存设置</button>} />
    <div className="settings-tabs">{tabs.map(t => <button data-active={tab === t.id} onClick={() => setTab(t.id)} key={t.id}>{t.label}</button>)}</div>
    <section className="settings-sheet">
      {tab === "selection" && <>
        <SettingRow title="启用划词翻译" description="在其他应用中选中文字后提供轻量翻译工具。"><Toggle label="启用划词翻译" checked={s.selection.enabled} onChange={enabled => update({ ...s, selection: { ...s.selection, enabled } })} /></SettingRow>
        <SettingRow title="划词触发方式" description="自动模式会在拖选结束后立即显示按钮，无需按键。"><Segmented value={s.selection.trigger_mode} options={[{ label: "快捷键", value: "hotkey" }, { label: "自动出现", value: "automatic" }]} onChange={trigger_mode => update({ ...s, selection: { ...s.selection, trigger_mode } })} /></SettingRow>
        {s.selection.trigger_mode === "hotkey" && <SettingRow title="划词触发快捷键" description="至少包含一个修饰键；Ctrl+Win+L 保留给 Windows 实时字幕。"><HotkeyRecorder value={s.selection.hotkey} onChange={hotkey => update({ ...s, selection: { ...s.selection, hotkey } })} /></SettingRow>}
        <SettingRow title="剪贴板回退" description="无法通过 UI Automation 读取选区时，允许短暂读取剪贴板。"><Toggle label="启用回退" checked={s.selection.clipboard_fallback_enabled} onChange={clipboard_fallback_enabled => update({ ...s, selection: { ...s.selection, clipboard_fallback_enabled } })} /></SettingRow>
      </>}
      {tab === "captions" && <>
        <div className="settings-group"><div className="settings-group-title"><div><h2>字幕来源</h2><p>运行中切换会先检查新来源，成功后开启全新会话。</p></div>{props.status.caption_state === "switching" && <span><Loader2 className="spin" />正在切换字幕来源</span>}</div>
          <div className="source-grid">
            <SourceCard title="Windows Live Caption" description="读取 Windows 系统字幕；不占用 GPU。" state="系统组件" selected={s.captions.source.type === "windows_live_caption"} onClick={() => void props.onSelectSource({ type: "windows_live_caption" })} />
            {props.models.map(model => <SourceCard key={model.id} title={model.display_name} description={model.recommended ? "日语优化、速度优先，推荐默认使用。" : "复杂音频精度优先，显存占用更高。"} state={modelStatusText(model.status)} badge={model.recommended ? "推荐" : undefined} selected={s.captions.source.type === "local_asr" && s.captions.source.model_id === model.id} onClick={() => void props.onSelectSource({ type: "local_asr", model_id: model.id, device: "cuda", compute_type: "int8_float16", vad_profile: s.captions.audio_mode })} />)}
          </div>
        </div>
        <ModelManager models={props.models} activeModel={props.status.active_model} refresh={props.refreshModels} message={props.onMessage} />
        <SettingRow title="翻译上下文" description="上下文来自已接受的 final 原文，不受上一条 LLM 成败影响。"><Segmented value={String(s.captions.context_segments)} options={[0, 1, 2, 4].map(value => ({ label: `${value} 条`, value: String(value) }))} onChange={value => update({ ...s, captions: { ...s.captions, context_segments: Number(value) } })} /></SettingRow>
        {s.captions.source.type === "local_asr" && <>
          <SettingRow title="音频模式" description="ASMR 会降低 VAD 阈值、延长静音提交时间，保留更多轻声。"><Segmented value={s.captions.audio_mode} options={[{ label: "普通", value: "normal" }, { label: "ASMR", value: "asmr" }]} onChange={audio_mode => update({ ...s, captions: { ...s.captions, audio_mode, source: { type: "local_asr", model_id: s.captions.source.type === "local_asr" ? s.captions.source.model_id : "kotoba-whisper-v2.0-faster", device: "cuda", compute_type: "int8_float16", vad_profile: audio_mode } } })} /></SettingRow>
          <Field label="模型镜像" hint="官方 Hugging Face 连接失败后使用；两条线路执行相同 SHA-256 校验。"><input value={s.captions.model_mirror_url} onChange={e => update({ ...s, captions: { ...s.captions, model_mirror_url: e.target.value } })} /></Field>
        </>}
        {s.captions.source.type === "windows_live_caption" && <>
          <SettingRow title="自动启动 Live Captions" description="开始翻译时自动打开 Windows 系统字幕。"><Toggle label="自动启动" checked={s.captions.auto_launch} onChange={auto_launch => update({ ...s, captions: { ...s.captions, auto_launch } })} /></SettingRow>
          <div className="three-cols"><Field label="轮询间隔 (ms)"><input type="number" min="80" value={s.captions.poll_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, poll_milliseconds: Number(e.target.value) } })} /></Field><Field label="稳定时间 (ms)"><input type="number" min="100" value={s.captions.stable_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, stable_milliseconds: Number(e.target.value) } })} /></Field><Field label="最长分段 (ms)"><input type="number" min="500" value={s.captions.max_duration_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, max_duration_milliseconds: Number(e.target.value) } })} /></Field></div>
        </>}
      </>}
      {tab === "llm" && <>
        <Field label="Endpoint"><input value={s.llm.endpoint} onChange={e => update({ ...s, llm: { ...s.llm, endpoint: e.target.value } })} /></Field>
        <div className="two-cols"><Field label="Model"><input value={s.llm.model} onChange={e => update({ ...s, llm: { ...s.llm, model: e.target.value } })} /></Field><Field label="目标语言"><input value={s.llm.target_language} onChange={e => update({ ...s, llm: { ...s.llm, target_language: e.target.value } })} /></Field></div>
        <Field label="API Key" hint={view.api_key_configured ? "已安全保存在 Windows 凭据管理器" : "尚未保存"}><div className="input-actions"><input type="password" placeholder={view.api_key_configured ? "输入新 Key 可替换" : "sk-..."} value={apiKey} onChange={e => setApiKey(e.target.value)} />{view.api_key_configured && <button title="删除 API Key" onClick={async () => { await deleteApiKey(); setApiKey(""); location.reload(); }}><Trash2 /></button>}</div></Field>
        <SettingRow title="启用思考" description="向兼容接口发送 enable_thinking=true；Extra Body 可覆盖它。"><Toggle label="启用思考" checked={s.llm.thinking_enabled} onChange={thinking_enabled => update({ ...s, llm: { ...s.llm, thinking_enabled } })} /></SettingRow>
        <Field label={`实时字幕超时 · ${(s.llm.timeout_milliseconds / 1000).toFixed(1)} 秒`} hint="仅限制实时字幕；划词翻译不设总时限"><input type="range" min="1000" max="5000" step="250" value={Math.min(s.llm.timeout_milliseconds, 5000)} onChange={e => update({ ...s, llm: { ...s.llm, timeout_milliseconds: Number(e.target.value) } })} /></Field>
        <Field label="Extra Body JSON"><textarea value={s.llm.extra_body_json} placeholder="{}" onChange={e => update({ ...s, llm: { ...s.llm, extra_body_json: e.target.value } })} /></Field>
        <button className="ghost test-button" onClick={props.onTest} disabled={props.testing}>{props.testing ? <Loader2 className="spin" /> : <BrainCircuit />}测试 LLM 连接</button>
      </>}
      {tab === "appearance" && <>
        <SettingRow title="字幕浮窗拖动" description="选择最顺手的移动方式；拖栏模式可避免误拖。"><Segmented value={s.overlay.drag_mode} options={[{ label: "Alt + 拖动", value: "alt" }, { label: "整体拖动", value: "anywhere" }, { label: "顶部拖栏", value: "handle" }]} onChange={drag_mode => update({ ...s, overlay: { ...s.overlay, drag_mode } })} /></SettingRow>
        <SettingRow title="主题" description="跟随系统或固定使用浅色、深色主题。"><Segmented value={s.visual.theme} options={[{ label: "系统", value: "system" }, { label: "浅色", value: "light" }, { label: "深色", value: "dark" }]} onChange={theme => update({ ...s, visual: { ...s.visual, theme } })} /></SettingRow>
        <SettingRow title="高斯模糊" description="为字幕和划词浮窗启用半透明模糊背景。"><Toggle label="高斯模糊" checked={s.visual.blur_enabled} onChange={blur_enabled => update({ ...s, visual: { ...s.visual, blur_enabled } })} /></SettingRow>
        <SettingRow title="减少动画" description="关闭页面切换和浮窗展开动画。"><Toggle label="减少动画" checked={s.visual.reduce_motion} onChange={reduce_motion => update({ ...s, visual: { ...s.visual, reduce_motion } })} /></SettingRow>
        <Field label={`字幕字号 · ${s.overlay.font_size}px`}><input type="range" min="14" max="34" value={s.overlay.font_size} onChange={e => update({ ...s, overlay: { ...s.overlay, font_size: Number(e.target.value) } })} /></Field>
        <SettingRow title="字幕背景" description="透明模式只保留字幕文字和悬浮控制区。"><Toggle label="透明字幕背景" checked={s.overlay.transparent} onChange={transparent => update({ ...s, overlay: { ...s.overlay, transparent } })} /></SettingRow>
        <SettingRow title="字幕颜色" description="也可以直接在字幕浮窗右下角随时更换。"><label className="color-control"><input type="color" value={s.overlay.caption_color} onChange={e => update({ ...s, overlay: { ...s.overlay, caption_color: e.target.value } })} /><span>{s.overlay.caption_color.toUpperCase()}</span></label></SettingRow>
      </>}
      {tab === "logs" && <>
        <div className="log-toolbar"><label><Search /><input value={props.logQuery} onChange={e => props.setLogQuery(e.target.value)} placeholder="搜索日志" /></label><button className="ghost" onClick={() => void openLogsDir()}><ExternalLink />目录</button><button className="ghost" onClick={props.onClearLogs}><Eraser />清空</button></div>
        <div className="log-view">{filteredLogs.length ? [...filteredLogs].reverse().map((l, i) => <div className="log-line" key={`${l.timestamp}-${i}`}><span data-level={l.level}>{l.level.toUpperCase()}</span><time>{new Date(l.timestamp).toLocaleTimeString()}</time><p>{l.message}</p></div>) : <EmptyState icon={<FileText />} title="日志很安静" text="运行状态和错误信息会显示在这里。" />}</div>
      </>}
    </section>
  </div>;
}

function SourceCard({ title, description, state, badge, selected, onClick }: { title: string; description: string; state: string; badge?: string; selected: boolean; onClick: () => void }) {
  return <button className="source-card" data-selected={selected} onClick={onClick}>
    <span className="source-card-top"><strong>{title}</strong>{badge && <i>{badge}</i>}</span>
    <p>{description}</p><small>{selected ? <Check /> : null}{state}</small>
  </button>;
}

function modelStatusText(status: ModelInfo["status"]) {
  const labels: Record<ModelInfo["status"], string> = { not_installed: "未安装", downloading: "下载中", verifying: "校验中", available: "可用", loading: "加载中", active: "活动中", corrupt: "损坏", incompatible: "不兼容", failed: "失败" };
  return labels[status];
}

function formatBytes(bytes: number) {
  if (!bytes) return "0 MB";
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(2)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
}

function ModelManager({ models, activeModel, refresh, message }: { models: ModelInfo[]; activeModel?: string; refresh: () => void; message: (text: string) => void }) {
  const [busy, setBusy] = useState<Record<string, string>>({});
  const action = async (modelId: string, label: string, run: () => Promise<unknown>) => {
    setBusy(current => ({ ...current, [modelId]: label }));
    message(`正在${label}中…`);
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    try {
      const result = await run();
      message(result && typeof result === "number" ? `${label}完成，dry-run ${result}ms` : `${label}已开始`);
      refresh();
    } catch (error) { message(String(error)); }
    finally { setBusy(current => { const next = { ...current }; delete next[modelId]; return next; }); }
  };
  return <div className="settings-group model-manager">
    <div className="settings-group-title"><div><h2>模型管理</h2><p>固定 revision、断点续传并强制校验 SHA-256。</p></div><div><button className="ghost" onClick={() => void openModelsDir()}><ExternalLink />模型目录</button><button className="ghost" onClick={refresh}><RefreshCw />重新检查</button></div></div>
    {models.map(model => {
      const progress = model.expected_size_bytes ? Math.min(100, model.downloaded_bytes / model.expected_size_bytes * 100) : 0;
      const installed = model.status === "available" || model.status === "active";
      const busyLabel = busy[model.id];
      return <article className="model-row" key={model.id} data-status={model.status}>
        <div className="model-copy"><div><strong>{model.display_name}</strong><span>{modelStatusText(model.status)}</span></div><p>{model.repository} · {model.license} · {formatBytes(model.expected_size_bytes)}</p>{model.status === "downloading" && <div className="download-progress"><i style={{ width: `${progress}%` }} /><small>{progress.toFixed(1)}% · {formatBytes(model.downloaded_bytes)}</small></div>}{model.error && <small className="model-error">{model.error}</small>}</div>
        <div className="model-actions">
          {busyLabel ? <button className="ghost" disabled><Loader2 className="spin" />正在{busyLabel}</button> : model.status === "downloading" ? <button className="ghost" onClick={() => void action(model.id, "取消下载", () => cancelModelDownload(model.id))}>暂停/取消</button> : !installed ? <button className="primary" onClick={() => void action(model.id, model.status === "not_installed" ? "下载" : "重试", () => downloadModel(model.id))}><Download />{model.status === "not_installed" ? "下载" : "重试"}</button> : <><button className="ghost" onClick={() => void action(model.id, "校验", () => verifyModel(model.id))}>校验</button><button className="ghost" disabled={activeModel === model.id} onClick={() => void action(model.id, "测试", () => testModel(model.id))}>测试</button><button className="ghost danger-text" disabled={activeModel === model.id} onClick={() => { if (confirm(`删除 ${model.display_name}？`)) void action(model.id, "删除", () => deleteModel(model.id)); }}><Trash2 /></button></>}
        </div>
      </article>;
    })}
  </div>;
}

function OverlayWindow() {
  const [items, setItems] = useState<CaptionTranslatedEvent[]>([]); const [currentSource, setCurrentSource] = useState(""); const [health, setHealth] = useState<CaptionSourceHealth>("unknown"); const [expanded, setExpanded] = useState(false); const [settings, setSettings] = useState<AppSettings>(); const [controls, setControls] = useState(false); const [refreshing, setRefreshing] = useState(false); const feedRef = useRef<HTMLDivElement>(null); const positionTimer = useRef<number | undefined>(undefined);
  useEffect(() => { void getSettings().then(v => setSettings(v.settings)); void getRuntimeStatus().then(v => setHealth(v.source_health)); const ps = [listen<string>("caption:source", e => setCurrentSource(e.payload)), listen<CaptionTranslatedEvent>("caption:translated", e => setItems(v => [...v, e.payload].slice(-12))), listen<CaptionSourceHealth>("caption:health-changed", e => setHealth(e.payload)), listen<SettingsView>("settings:changed", e => setSettings(e.payload.settings))]; const moved = currentWindow.onMoved(e => { if (positionTimer.current) clearTimeout(positionTimer.current); positionTimer.current = window.setTimeout(() => void saveOverlayPosition(e.payload.x, e.payload.y), 180); }); return () => { if (positionTimer.current) clearTimeout(positionTimer.current); ps.forEach(p => void p.then(u => u())); void moved.then(u => u()); }; }, []);
  useEffect(() => { const feed = feedRef.current; if (feed) requestAnimationFrame(() => feed.scrollTo({ top: feed.scrollHeight, behavior: "smooth" })); }, [items, expanded]);
  useTheme(settings);
  const toggleExpanded = async () => { const next = !expanded; setExpanded(next); await currentWindow.setSize(new LogicalSize(settings?.overlay.width ?? 760, next ? 320 : 170)); };
  const persistOverlay = async (patch: Partial<AppSettings["overlay"]>) => { if (!settings) return; const next = { ...settings, overlay: { ...settings.overlay, ...patch } }; setSettings(next); try { await saveSettings(next); } catch { /* keep controls responsive */ } };
  const refresh = async () => { if (refreshing) return; setRefreshing(true); try { await refreshCaption(); } finally { setRefreshing(false); } };
  const drag = (e: React.MouseEvent, fromHandle = false) => { if (e.button !== 0 || (e.target as HTMLElement).closest("button,input")) return; const mode = settings?.overlay.drag_mode ?? "alt"; if ((mode === "alt" && e.altKey) || mode === "anywhere" || (mode === "handle" && fromHandle)) void currentWindow.startDragging(); };
  const dragHint = settings?.overlay.drag_mode === "anywhere" ? "拖动任意位置" : settings?.overlay.drag_mode === "handle" ? "拖动此处" : "ALT + 拖动";
  return <main className="overlay-window floating" data-expanded={expanded} data-transparent={settings?.overlay.transparent} style={{ fontSize: settings?.overlay.font_size, "--caption-color": settings?.overlay.caption_color } as React.CSSProperties} onMouseDown={e => drag(e)}>
    <header className="overlay-bar" onMouseDown={e => drag(e, true)}><span><i data-health={health} />实时翻译</span><div><small><GripHorizontal />{dragHint}</small><button title="刷新字幕连接" disabled={refreshing} onClick={() => void refresh()}>{refreshing ? <Loader2 className="spin" /> : <RefreshCw />}</button><button title={expanded ? "收起字幕记录" : "展开字幕记录"} onClick={() => void toggleExpanded()}>{expanded ? <ChevronUp /> : <ChevronDown />}</button><button className="overlay-close" title="结束实时字幕" onClick={() => void setCaptionRunning(false)}><X /></button></div></header>
    <div className="caption-feed" ref={feedRef}>
      {items.map(item => <section key={item.segment.id}><span>{item.segment.source_text}</span><p>{item.result.translated_text || item.result.error}</p></section>)}
      {!items.length && <p className="muted waiting-caption">{currentSource ? "正在整理第一句翻译…" : "等待实时字幕…"}</p>}
    </div>
    {currentSource && <aside className="recognition-dock"><span><i />实时识别</span><p title={currentSource}>{currentSource}</p></aside>}
    <div className="overlay-quick"><button data-active={controls} title="字幕外观" onClick={() => setControls(v => !v)}><Palette /></button>{controls && <div className="overlay-popover"><div><span>背景</span><button className="mini-toggle" data-active={settings?.overlay.transparent} onClick={() => void persistOverlay({ transparent: !settings?.overlay.transparent })}>{settings?.overlay.transparent ? "透明" : "毛玻璃"}</button></div><label><span>字幕颜色</span><input type="color" value={settings?.overlay.caption_color ?? "#ffffff"} onChange={e => void persistOverlay({ caption_color: e.target.value })} /></label></div>}</div>
  </main>;
}

function ToolbarWindow() {
  const [selection, setSelection] = useState<SelectionReadyEvent>(); const [pending, setPending] = useState(false); const [result, setResult] = useState<TranslationResult>(); const [busy, setBusy] = useState(false); const [expanded, setExpanded] = useState(false); const [copied, setCopied] = useState(false); const [showOriginal, setShowOriginal] = useState(true); const [settings, setSettings] = useState<AppSettings>(); const copiedTimer = useRef<number | undefined>(undefined);
  useEffect(() => { void getSettings().then(v => setSettings(v.settings)); const ps = [listen("selection:pending", async () => { setSelection(undefined); setPending(true); setResult(undefined); setExpanded(false); setCopied(false); await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44)); }), listen("selection:cancelled", async () => { setPending(false); await currentWindow.hide(); }), listen<SelectionReadyEvent>("selection:ready", async e => { setSelection(e.payload); setPending(false); setResult(undefined); setExpanded(false); setCopied(false); await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44)); }), listen<string>("translation:delta", e => setResult(v => ({ ...(v ?? emptyResult), translated_text: `${v?.translated_text ?? ""}${e.payload}` }))), listen<SettingsView>("settings:changed", e => { setSettings(e.payload.settings); setResult(undefined); })]; return () => { if (copiedTimer.current) clearTimeout(copiedTimer.current); ps.forEach(p => void p.then(u => u())); }; }, []);
  useTheme(settings);
  const translate = async () => {
    if (!selection || !settings) return;
    setExpanded(true); setBusy(true); setResult(undefined);
    try { await currentWindow.setFocusable(true); await currentWindow.setSize(new LogicalSize(520, 340)); await currentWindow.show(); await currentWindow.setFocus(); } catch { /* translation must still proceed */ }
    try { setResult(await translateSelection({ mode: "selection", source_text: selection.text, source_language: "auto", target_language: settings.llm.target_language })); }
    catch (error) { setResult({ ...emptyResult, source_text: selection.text, error: String(error) }); }
    finally { setBusy(false); }
  };
  const copyText = async (text: string) => { await navigator.clipboard.writeText(text); setCopied(true); if (copiedTimer.current) clearTimeout(copiedTimer.current); copiedTimer.current = window.setTimeout(() => setCopied(false), 1000); };
  const collapse = async () => { setExpanded(false); setResult(undefined); try { await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44)); } catch { /* keep the toolbar usable */ } };
  if (!expanded) return <main className="selection-toolbar floating">
    <button className="toolbar-action primary-action" disabled={pending || !selection} onClick={() => void translate()}>{pending ? <Loader2 className="spin" /> : <Languages />}<span>翻译</span></button>
    <button className="toolbar-action" disabled={pending || !selection} onClick={() => selection && void copyText(selection.text)}>{copied ? <Check /> : <Copy />}<span>{copied ? "已复制" : "复制"}</span></button>
    <button className="toolbar-action close-action" onClick={() => void currentWindow.hide()}><X /><span>关闭</span></button>
  </main>;
  return <main className="selection-result floating">
    <header className="result-titlebar" onMouseDown={e => { if (e.button === 0 && !(e.target as HTMLElement).closest("button")) void currentWindow.startDragging(); }}><div><Languages /><strong>翻译</strong></div><nav><button title="收起" onClick={e => { e.stopPropagation(); void collapse(); }}><Minus /></button><button title="关闭" onClick={e => { e.stopPropagation(); void currentWindow.hide(); }}><X /></button></nav></header>
    <div className="language-row"><span>自动检测</span><ArrowRight /><strong>{settings?.llm.target_language}</strong><button onClick={() => setShowOriginal(v => !v)}>{showOriginal ? "隐藏原文" : "显示原文"}</button></div>
    <section className="result-content">
      {showOriginal && <p className="source-text">{selection?.text}</p>}
      {busy && !result?.translated_text ? <div className="result-loading"><Loader2 className="spin" /><span>正在连接模型…</span></div> : <p className="translated-text" data-streaming={busy}>{result?.translated_text || result?.error}</p>}
    </section>
    <footer className="result-footer"><small>{result?.model ? `${result.model} · 首字 ${result.first_token_ms}ms · 完成 ${result.latency_ms}ms` : ""}</small><div><button disabled={busy} onClick={() => void translate()}><RotateCcw />重新生成</button><button disabled={busy || !result?.translated_text} onClick={() => result && void copyText(result.translated_text)}>{copied ? <Check /> : <Copy />}{copied ? "已复制" : "复制"}</button></div></footer>
  </main>;
}

function Logo() { return <div className="logo" title="LiveCaption"><svg viewBox="0 0 48 48" aria-hidden="true"><path d="M8 9h25a7 7 0 0 1 7 7v12a7 7 0 0 1-7 7H21l-8 7v-7H8a6 6 0 0 1-6-6V15a6 6 0 0 1 6-6Z" /><path d="M16 18h15M16 25h10" /></svg></div>; }
function RailButton({ active, label, onClick, children }: { active: boolean; label: string; onClick: () => void; children: React.ReactNode }) { return <button className="rail-button" data-active={active} title={label} onClick={onClick}>{children}<span>{label}</span></button>; }
function StatusDot({ on, text }: { on: boolean; text: string }) { return <div className="status-dot" data-on={on}><i />{text}</div>; }
function SettingRow({ title, description, children }: { title: string; description: string; children: React.ReactNode }) { return <div className="setting-row"><div><h3>{title}</h3><p>{description}</p></div>{children}</div>; }
function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) { return <label className="field"><span>{label}{hint && <small>{hint}</small>}</span>{children}</label>; }
function ResultBlock({ result }: { result: TranslationResult }) { return <article className="result-block"><div><span>{result.source_text}</span><button onClick={() => void navigator.clipboard.writeText(result.translated_text)}><Copy /></button></div><p>{result.translated_text}</p><small>{result.model} · {result.latency_ms}ms</small></article>; }
function EmptyState({ icon, title, text }: { icon: React.ReactNode; title: string; text: string }) { return <div className="empty-state">{icon}<h3>{title}</h3><p>{text}</p></div>; }
function HotkeyRecorder({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [recording, setRecording] = useState(false);
  return <button type="button" className="hotkey-input" data-recording={recording} title="点击后按下新的组合键" onFocus={() => setRecording(true)} onBlur={() => setRecording(false)} onKeyDown={e => {
    e.preventDefault();
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    if (!e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) return;
    if (e.ctrlKey && e.metaKey && e.code === "KeyL") return;
    const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super", e.code].filter(Boolean);
    onChange(parts.join("+")); setRecording(false); e.currentTarget.blur();
  }}>{recording ? "请按组合键…" : displayHotkey(value)}</button>;
}
function displayHotkey(value: string) { return value.replace(/Key([A-Z])/g, "$1").replace(/Digit([0-9])/g, "$1"); }
