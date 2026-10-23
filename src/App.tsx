import { LlmProfiles } from "./components/LlmProfiles";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import {
  Activity, ArrowLeft, ArrowRight, BookMarked, BrainCircuit, Captions, Check, ChevronLeft, ChevronRight,
  Clipboard, Copy, Download, Eraser, ExternalLink, FileText, Languages, Loader2, MousePointer2,
  Play, Plus, RefreshCw, RotateCcw, Search, Settings, Sparkles, Square, Trash2, Upload, Wand2, X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Segmented } from "./components/Segmented";
import { Slider } from "./components/Slider";
import { Toggle } from "./components/Toggle";
import { OverlayWindow } from "./windows/OverlayWindow";
import { ToolbarWindow } from "./windows/ToolbarWindow";
import { OcrCaptureWindow, OcrResultWindow, startOcrCapture } from "./windows/OcrWindow";
import {
  clearRuntimeLogs, correctCaptionSession, deleteCaptionSession, exportCaptionSession,
  exportGlossary, getGlossary, getRuntimeLogs, getRuntimeStatus, getSettings, importGlossary,
  listCaptionSessions, newCaptionSession, openLogsDir, openSessionsDir, readCaptionSession, resetAsrChannelRouting,
  saveGlossary, saveSettings, setCaptionRunning, testLlm, listModels, downloadModel, cancelModelDownload,
  verifyModel, testModel, deleteModel, openModelsDir, switchCaptionSource,
  checkAsrDependencies, openAsrDependencyUrl, downloadAsrGpuRuntime, cancelAsrGpuRuntime, testDownloadProxy,
} from "./lib/api";
import type {
  AppSettings, AsmrChannelState, AsrDependencyReport, AsrGpuRuntimeInfo, AsrGpuRuntimeProgressEvent, CaptionRuntimeState, CaptionSourceConfig, CaptionSourceHealth,
  CorrectionProgress, Glossary, ModelInfo, ModelProgressEvent, RuntimeLogEntry, RuntimeStatus,
  SessionPageView, SessionSummary, SettingsView, TranslationResult,
} from "./lib/contracts";
import { displayHotkey, emptyResult, previewSettings, useTheme } from "./lib/ui-shared";
import { currentWindow, isTauri, windowLabel } from "./lib/window";

document.documentElement.dataset.window = windowLabel;
const bootTheme = localStorage.getItem("livecaption-theme") ?? "system";
document.documentElement.dataset.theme = bootTheme === "system"
  ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
  : bootTheme;

const builtInModels: ModelInfo[] = [
  {
    kind: "asr", id: "kotoba-whisper-v2.0-faster", display_name: "Kotoba Whisper v2.0 Faster",
    repository: "kotoba-tech/kotoba-whisper-v2.0-faster", revision: "f44edd35eaeb2274e85ac7b31fb2c6f59ff1c4bc",
    license: "MIT", expected_size_bytes: 1_516_480_096, installed_size_bytes: 0,
    status: "not_installed", downloaded_bytes: 0, recommended: true,
  },
  {
    kind: "asr", id: "whisper-large-v3-turbo", display_name: "Whisper large-v3-turbo",
    repository: "dropbox-dash/faster-whisper-large-v3-turbo", revision: "0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf",
    license: "MIT", expected_size_bytes: 1_621_665_983, installed_size_bytes: 0,
    status: "not_installed", downloaded_bytes: 0, recommended: false,
  },
  {
    kind: "ocr", id: "ocr-ppocr-v5-mobile", display_name: "PP-OCRv5 Mobile · 截图识别",
    repository: "RapidAI/RapidOCR", revision: "v3.10.0", license: "Apache-2.0",
    expected_size_bytes: 22_469_390, installed_size_bytes: 0,
    status: "not_installed", downloaded_bytes: 0, recommended: true,
  },
];

export function App() {
  if (windowLabel === "ocr-capture") return <OcrCaptureWindow />;
  if (windowLabel === "ocr-result") return <OcrResultWindow />;
  if (windowLabel === "overlay") return <OverlayWindow />;
  if (windowLabel === "selection-toolbar") return <ToolbarWindow />;
  return <MainWindow />;
}

type Page = "selection" | "captions" | "glossary" | "settings";
type SettingsTab = "features" | "resources" | "application" | "appearance";

// previews mirror the backend convention: newest session first
const demoSessions: SessionSummary[] = [
  { id: "2026-09-01_14-02-55", created_at: "2026-09-01T14:02:55+08:00", entry_count: 45, first_source: "今日のテーマは先週の続きです", first_translation: "今天的主题是上周的继续", has_correction: true, is_current: true },
  { id: "2026-08-31_22-40-15", created_at: "2026-08-31T22:40:15+08:00", entry_count: 23, first_source: "それでは始めましょう", first_translation: "那么开始吧", has_correction: false, is_current: false },
  { id: "2026-08-31_18-10-44", created_at: "2026-08-31T18:10:44+08:00", entry_count: 132, first_source: "では、次のコーナーです", first_translation: "那么进入下一个环节", has_correction: true, is_current: false },
  { id: "2026-08-30_21-12-03", created_at: "2026-08-30T21:12:03+08:00", entry_count: 87, first_source: "おはようございます、皆さん", first_translation: "大家早上好", has_correction: true, is_current: false },
  { id: "2026-08-30_13-01-26", created_at: "2026-08-30T13:01:26+08:00", entry_count: 201, first_source: "はい、こんにちは", first_translation: "大家好", has_correction: false, is_current: false },
  { id: "2026-08-29_20-05-11", created_at: "2026-08-29T20:05:11+08:00", entry_count: 54, first_source: "次の動画はこちらです", first_translation: "下一个视频在这里", has_correction: true, is_current: false },
  { id: "2026-08-28_23-30-58", created_at: "2026-08-28T23:30:58+08:00", entry_count: 9, first_source: "テスト中です", first_translation: "正在测试中", has_correction: false, is_current: false },
  { id: "2026-08-27_19-44-07", created_at: "2026-08-27T19:44:07+08:00", entry_count: 64, first_source: "歌枠です", first_translation: "是歌回直播", has_correction: false, is_current: false },
  { id: "2026-08-26_22-08-31", created_at: "2026-08-26T22:08:31+08:00", entry_count: 76, first_source: "みなさん、こんばんは", first_translation: "大家晚上好", has_correction: false, is_current: false },
];

function formatSessionDateTime(iso: string) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "时间未知";
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function formatEntryTime(iso: string) {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleTimeString("zh-CN", { hour12: false });
}

function MainWindow() {
  const switchingSource = useRef(false);
  const [settingsAnchor, setSettingsAnchor] = useState<string>();
  const [view, setView] = useState<SettingsView | null>(null);
  const [bootError, setBootError] = useState("");
  // browser preview may deep-link a page with ?page=captions|glossary|settings
  const [page, setPage] = useState<Page>(() => {
    const initial = new URLSearchParams(location.search).get("page");
    return initial === "captions" || initial === "glossary" || initial === "settings" ? initial : "selection";
  });
  const [settingsTab, setSettingsTab] = useState<SettingsTab>(() => { const saved = localStorage.getItem("settings-category"); return saved === "resources" || saved === "application" || saved === "features" ? saved : "appearance"; });
  const [status, setStatus] = useState<RuntimeStatus>({ caption_running: false, caption_state: "stopped", selection_hotkey_registered: false, source_health: "unknown", last_caption_update_ms: 0, reader_restarts: 0, source_error_count: 0, translation_queue_depth: 0, last_translation_latency_ms: 0, last_translation_first_token_ms: 0, selected_source: previewSettings.captions.source, asr_latency_ms: 0, asmr_channel_state: "inactive" });
  const [models, setModels] = useState<ModelInfo[]>(builtInModels);
  const [message, setMessage] = useState("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [selectionResult, setSelectionResult] = useState<TranslationResult>(emptyResult);
  const [sessions, setSessions] = useState<SessionSummary[]>(isTauri ? [] : demoSessions);
  const [viewingSession, setViewingSession] = useState<string | null>(null);
  const [logs, setLogs] = useState<RuntimeLogEntry[]>([]);
  const [logQuery, setLogQuery] = useState("");
  const sessionRefreshTimer = useRef<number | undefined>(undefined);

  const refreshSessions = useCallback(async () => {
    if (!isTauri) return;
    try { setSessions(await listCaptionSessions()); } catch { /* the ticker is optional chrome */ }
  }, []);
  const scheduleSessionRefresh = useCallback(() => {
    if (sessionRefreshTimer.current) return;
    sessionRefreshTimer.current = window.setTimeout(() => {
      sessionRefreshTimer.current = undefined;
      void refreshSessions();
    }, 2500);
  }, [refreshSessions]);

  const selectSession = useCallback((id: string | null) => { setViewingSession(id); setPage("captions"); }, []);

  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(() => setMessage(""), 5_000);
    return () => window.clearTimeout(timer);
  }, [message]);

  const loadInitialSettings = useCallback(async () => {
    setBootError("");
    const watchdog = window.setTimeout(() => {
      setBootError("初始化等待时间过长。后台仍在运行，你可以重试加载界面。");
    }, 4_000);
    try {
      setView(await getSettings());
    } catch (error) {
      if (!("__TAURI_INTERNALS__" in window)) {
        setView({ settings: previewSettings, api_key_configured: false, model_directory: "E:\\Personal\\个人兴趣\\Livecaption\\Model" });
      } else {
        setBootError(`读取配置失败：${String(error)}`);
      }
    } finally {
      clearTimeout(watchdog);
    }
  }, []);

  const refreshModels = useCallback(async (showError = false) => {
    try {
      setModels(await listModels());
      return true;
    } catch (error) {
      if (showError) setMessage(`检查本地模型失败：${String(error)}`);
      return false;
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
    if (!isTauri) { void loadInitialSettings(); return; }
    void getRuntimeStatus().then(setStatus).catch(() => undefined);
    const unsubs = [
      listen<void>("ocr:resources", () => { setSettingsAnchor("ocr-models"); setPage("settings"); setSettingsTab("resources"); }),
      listen<string>("ocr:error", e => setMessage(e.payload)),
      listen<SettingsView>("llm:profiles-changed", e => setView(current => current ? mergeProfileView(current, e.payload) : e.payload)),
      listen<SettingsView>("settings:changed", e => { if (!switchingSource.current) setView(e.payload); }),
      listen<TranslationResult>("translation:finished", e => setSelectionResult(e.payload)),
      listen("caption:translated", () => scheduleSessionRefresh()),
      listen<void>("caption:sessions-changed", () => void refreshSessions()),
      listen<CaptionRuntimeState>("caption:state-changed", e => setStatus(s => ({ ...s, caption_state: e.payload, caption_running: ["running", "starting", "loading_model", "switching"].includes(e.payload) }))),
      listen<CaptionSourceHealth>("caption:health-changed", e => setStatus(s => ({ ...s, source_health: e.payload }))),
      listen<AsmrChannelState>("caption:asmr-channel-changed", e => setStatus(s => ({ ...s, asmr_channel_state: e.payload }))),
      listen<ModelProgressEvent>("model:progress", e => {
        const p = e.payload;
        setModels(items => items.map(item => item.id === p.model_id ? { ...item, status: p.status, downloaded_bytes: p.downloaded_bytes, error: p.error } : item));
        if (p.status === "failed") setMessage(`模型下载失败：${p.error ?? "请查看日志后重试"}`);
        else if (p.status === "available") setMessage("模型已下载并通过校验");
        if (p.status === "available" || p.status === "corrupt" || p.status === "failed" || p.status === "incompatible") void refreshModels();
      }),
      listen<void>("caption:session-reset", () => { setViewingSession(null); void refreshSessions(); }),
      listen<string>("runtime:error", e => setMessage(e.payload)),
    ];
    // Subscribe before loading so a legacy-profile migration cannot be missed
    // between the initial snapshot and listener registration.
    void Promise.all(unsubs).then(() => loadInitialSettings()).catch(() => loadInitialSettings());
    const interval = window.setInterval(() => void getRuntimeStatus().then(setStatus).catch(() => undefined), 1200);
    void refreshModels();
    void refreshSessions();
    const modelRefreshRetries = [600, 2_000].map(delay => window.setTimeout(() => void refreshModels(), delay));
    return () => {
      clearInterval(interval);
      modelRefreshRetries.forEach(clearTimeout);
      if (sessionRefreshTimer.current) clearTimeout(sessionRefreshTimer.current);
      unsubs.forEach(p => void p.then(u => u()));
    };
  }, [loadInitialSettings, refreshModels, refreshSessions, scheduleSessionRefresh]);

  useEffect(() => {
    if (!isTauri || settingsTab !== "application" || page !== "settings") return;
    const refresh = () => void getRuntimeLogs().then(setLogs).catch(() => undefined);
    refresh();
    const id = window.setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, [page, settingsTab]);

  useTheme(view?.settings);
  const settings = view?.settings;
  const update = useCallback((next: AppSettings) => setView(v => v ? { ...v, settings: next } : v), []);

  const persist = async () => {
    if (!settings) return;
    setSaving(true); setMessage("");
    try {
      const saved = await saveSettings(settings);
      setView(saved); setMessage("设置已保存");
    } catch (error) { setMessage(String(error)); }
    finally { setSaving(false); }
  };

  const runTest = async () => {
    if (!isTauri) { setMessage("预览环境不发送网络请求"); return; }
    setTesting(true); setMessage("");
    try { const result = await testLlm(); setSelectionResult(result); setMessage("LLM 连接正常"); }
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
        if (window.confirm(`未找到可用的 ${selected.display_name}（${size} GB）。\n\n状态：${modelStatusText(selected.status)}\n保存位置：${view.model_directory}\n来源：Hugging Face 官方源（失败后使用镜像）\n\n现在开始下载吗？`)) {
          try { await downloadModel(modelId); setMessage("模型下载已开始，可在设置 → 资源中查看进度"); }
          catch (error) { setMessage(String(error)); }
        }
        return;
      }
    }
    try { await setCaptionRunning(next); setStatus(s => ({ ...s, caption_running: next, caption_state: next ? "starting" : "stopping" })); }
    catch (error) { setMessage(String(error)); }
  };

  const selectSource = async (source: CaptionSourceConfig) => {
    if (switchingSource.current) return;
    if (!isTauri) {
      setView(current => current ? { ...current, settings: { ...current.settings, captions: { ...current.settings.captions, source } } } : current);
      return;
    }
    switchingSource.current = true;
    try {
      await switchCaptionSource(source);
      const saved = await getSettings();
      // Source selection applies immediately, but must not discard edits in other chapters.
      setView(current => current ? { ...current, settings: { ...current.settings, captions: { ...current.settings.captions, source: saved.settings.captions.source } } } : saved);
      setStatus(await getRuntimeStatus());
      setMessage(status.caption_running ? "正在切换字幕来源" : "字幕来源已更新");
    } catch (error) { setMessage(String(error)); }
    finally { switchingSource.current = false; }
  };

  if (!settings || !view) return (
    <main className="boot">
      {bootError
        ? <section className="boot-card"><Logo /><h2>界面尚未完成加载</h2><p>{bootError}</p><button className="primary" onClick={() => void loadInitialSettings()}><RefreshCw />重新加载</button></section>
        : <Loader2 className="spin" aria-label="正在加载" />}
    </main>
  );

  return (
    <MotionConfig reducedMotion={settings.visual.reduce_motion ? "always" : "never"}>
      <main className="shell">
        <aside className="rail">
          <Logo />
          <nav>
            <RailButton active={page === "selection"} label="划词翻译" onClick={() => setPage("selection")}><Clipboard /></RailButton>
            <RailButton active={page === "captions"} label="实时字幕" onClick={() => setPage("captions")}><Captions /></RailButton>
            <RailButton active={page === "glossary"} label="术语表" onClick={() => setPage("glossary")}><BookMarked /></RailButton>
            <RailButton active={page === "settings"} label="设置" onClick={() => setPage("settings")}><Settings /></RailButton>
          </nav>
          <span className="rail-spacer" />
          <span className="version">v0.5</span>
        </aside>

        <section className="content">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={page}
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8 }}
              transition={{ duration: .2, ease: [0.22, 1, 0.36, 1] }}
            >
              {page === "selection" && <SelectionPage settings={settings} status={status} result={selectionResult} />}
              {page === "captions" && <CaptionPage status={status} sessions={sessions} viewingSession={viewingSession} onSelectSession={selectSession} onToggle={toggleCaptions} onMessage={setMessage} />}
              {page === "glossary" && <GlossaryPage onMessage={setMessage} />}
              {page === "settings" && (
                <SettingsPage tab={settingsTab} setTab={setSettingsTab} anchor={settingsAnchor} onAnchorHandled={() => setSettingsAnchor(undefined)} view={view} onProfilesChanged={next => setView(current => current ? mergeProfileView(current, next) : next)}
                  update={update} onSave={persist} saving={saving} testing={testing} onTest={runTest}
                  models={models} status={status} onSelectSource={selectSource} refreshModels={() => void refreshModels(true)} onMessage={setMessage}
                  logs={logs} logQuery={logQuery} setLogQuery={setLogQuery}
                  onClearLogs={async () => { await clearRuntimeLogs(); setLogs([]); }} />
              )}
            </motion.div>
          </AnimatePresence>
          <AnimatePresence>
            {message && (
              <motion.div
                className="toast" role="status" aria-live="polite"
                initial={{ opacity: 0, y: 18, scale: .96 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 10, scale: .98 }}
                transition={{ type: "spring", stiffness: 420, damping: 30 }}
              >
                <span className="toast-icon"><Sparkles /></span>
                <span>{message}</span>
                <button aria-label="关闭提示" onClick={() => setMessage("")}><X size={14} /></button>
              </motion.div>
            )}
          </AnimatePresence>
        </section>
      </main>
    </MotionConfig>
  );
}

function PageHeader({ eyebrow, title, description, action }: { eyebrow: string; title: string; description: string; action?: React.ReactNode }) {
  return <header className="page-header"><div><span>{eyebrow}</span><h1>{title}</h1><p>{description}</p></div>{action}</header>;
}

function SelectionPage({ settings, status, result }: { settings: AppSettings; status: RuntimeStatus; result: TranslationResult }) {
  const automatic = settings.selection.trigger_mode === "automatic";
  const [ocrError, setOcrError] = useState("");
  return <div className="page">
    <PageHeader eyebrow="QUICK TRANSLATE" title="划词翻译" description={automatic ? "选中文字后，翻译按钮会立即出现在光标旁。" : "选中文字，用快捷键唤出轻巧的翻译按钮。"}
      action={<StatusDot on={status.selection_hotkey_registered} text={status.selection_hotkey_registered ? "服务已就绪" : "服务未启用"} />} />
    <section className="workflow-strip">
      <div className="workflow-icon">{automatic ? <MousePointer2 /> : <span>{displayHotkey(settings.selection.hotkey)}</span>}</div>
      <div><span className="overline">{automatic ? "自动感知选区" : "全局快捷键"}</span><h2>{automatic ? "松开鼠标，即刻出现" : `${displayHotkey(settings.selection.hotkey)} · 随时唤起`}</h2><p>浮动工具只在有效选区旁出现，不抢焦点，不打断当前应用。</p></div>
      <div className="flow-steps"><span>选择</span><ArrowRight /><span>点击翻译</span><ArrowRight /><span>查看结果</span></div>
    </section>
    <section className="workflow-strip"><div className="workflow-icon"><Languages /></div><div><h2>截图翻译</h2><p>框选图片或屏幕文字，识别后按文字块翻译。首次使用请在设置 → 资源下载 OCR 模型。</p>{ocrError && <p role="alert">{ocrError}</p>}</div><button className="primary" onClick={() => { setOcrError(""); void startOcrCapture().catch(e => setOcrError(String(e))); }}>识屏 · 截图翻译</button></section>
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
const asmrChannelStateText: Record<AsmrChannelState, string> = {
  inactive: "启动实时字幕后显示锁定状态。",
  searching: "正在分析左右声道并搜索可信人声。",
  locked_left: "左侧已锁定；句间静音不会解除。",
  locked_right: "右侧已锁定；句间静音不会解除。",
  locked_mix: "检测到两侧高度相关的中央人声。",
  pending_left: "正在确认人声是否已切换到左侧。",
  pending_right: "正在确认人声是否已切换到右侧。",
  fallback_mono: "设备不支持双声道采集，当前已退回单声道。",
};
const SESSION_PAGE_SIZE = 20;

const demoCaptionPage: SessionPageView = {
  total: 46, page: 3, page_count: 3,
  entries: [
    { index: 41, timestamp: "2026-09-01T14:24:10+08:00", source_text: "じゃあ、次のコーナーに行きましょう", translated_text: "那么进入下一个环节吧", model: "gpt-4o-mini", latency_ms: 812 },
    { index: 42, timestamp: "2026-09-01T14:24:26+08:00", source_text: "今日のゲストは桜井先生です", translated_text: "今天的嘉宾是樱井老师", corrected_source_text: "今日のゲストは桜井先生です", corrected_translated_text: "今天的嘉宾是桜井先生", model: "gpt-4o-mini", latency_ms: 764 },
    { index: 43, timestamp: "2026-09-01T14:24:41+08:00", source_text: "よろしくお願いします", translated_text: "请多关照", model: "gpt-4o-mini", latency_ms: 690 },
    { index: 44, timestamp: "2026-09-01T14:25:02+08:00", source_text: "まずは自己紹介からお願いします", translated_text: "首先请做一下自我介绍", model: "gpt-4o-mini", latency_ms: 845 },
    { index: 45, timestamp: "2026-09-01T14:25:20+08:00", source_text: "私は十年前からこの研究をしています", translated_text: "我从十年前就开始这项研究了", model: "gpt-4o-mini", latency_ms: 901 },
    { index: 46, timestamp: "2026-09-01T14:25:38+08:00", source_text: "えーと、資料を見てください", translated_text: "呃，请看一下资料", model: "gpt-4o-mini", latency_ms: 733 },
  ],
};

function CaptionPage({ status, sessions, viewingSession, onSelectSession, onToggle, onMessage }: {
  status: RuntimeStatus; sessions: SessionSummary[]; viewingSession: string | null;
  onSelectSession: (id: string | null) => void; onToggle: () => void; onMessage: (message: string) => void;
}) {
  const busy = status.caption_state === "starting" || status.caption_state === "stopping" || status.caption_state === "switching" || status.caption_state === "loading_model";
  const activeName = sourceName(status.active_source ?? status.selected_source);
  const actionText = busy ? stateText[status.caption_state] : status.caption_running ? "停止字幕" : "启动字幕";
  const currentSession = sessions.find(session => session.is_current);
  const activeId = viewingSession ?? currentSession?.id ?? null;
  const activeSummary = viewingSession ? sessions.find(session => session.id === viewingSession) : currentSession;
  const isViewingCurrent = viewingSession === null;
  const [pageView, setPageView] = useState<SessionPageView | null>(null);
  const [pageDraft, setPageDraft] = useState("1");
  const [showCorrected, setShowCorrected] = useState(true);
  const [exportFormat, setExportFormat] = useState("srt");
  const [correctionOpen, setCorrectionOpen] = useState(false);
  const [useGlossary, setUseGlossary] = useState(true);
  const [correction, setCorrection] = useState({ running: false, completed: 0, total: 0 });
  const followTimer = useRef<number | undefined>(undefined);
  const pageRef = useRef(1);

  const loadPage = useCallback(async (targetPage: number) => {
    if (!activeId) { setPageView(null); return; }
    if (!isTauri) {
      const pageCount = Math.max(1, Math.ceil(demoCaptionPage.total / SESSION_PAGE_SIZE));
      const page = Math.min(Math.max(1, targetPage), pageCount);
      pageRef.current = page;
      setPageView({ ...demoCaptionPage, page, page_count: pageCount });
      return;
    }
    try {
      const view = await readCaptionSession(activeId, targetPage, SESSION_PAGE_SIZE);
      pageRef.current = view.page;
      setPageView(view);
    } catch (error) { onMessage(String(error)); }
  }, [activeId, onMessage]);

  useEffect(() => {
    setCorrectionOpen(false); setCorrection({ running: false, completed: 0, total: 0 });
    // live with the current process's record: newest page; history starts at page 1
    void loadPage(isViewingCurrent ? Number.MAX_SAFE_INTEGER : 1);
  }, [loadPage, isViewingCurrent]);

  useEffect(() => { setPageDraft(String(pageView?.page ?? 1)); }, [pageView?.page]);

  const jumpToPage = () => {
    if (!pageView) return;
    const target = Number(pageDraft);
    if (Number.isInteger(target) && target >= 1 && target <= pageView.page_count) {
      if (target !== pageView.page) void loadPage(target);
    } else {
      setPageDraft(String(pageView.page));
    }
  };

  useEffect(() => {
    if (!isTauri) return;
    const unsubs = [
      listen("caption:translated", () => {
        if (!isViewingCurrent || followTimer.current) return;
        followTimer.current = window.setTimeout(() => {
          followTimer.current = undefined;
          void loadPage(Number.MAX_SAFE_INTEGER);
        }, 400);
      }),
      listen<CorrectionProgress>("correction:progress", e => {
        const progress = e.payload;
        if (progress.id !== activeId) return;
        if (progress.error) {
          setCorrection({ running: false, completed: 0, total: 0 });
          onMessage(`纠错失败：${progress.error}`);
          return;
        }
        setCorrection(current => current.running ? { running: true, completed: progress.completed, total: progress.total } : current);
      }),
      listen<string>("correction:finished", e => {
        if (e.payload !== activeId) return;
        setCorrection({ running: false, completed: 0, total: 0 });
        onMessage("字幕纠错完成");
        void loadPage(pageRef.current);
      }),
    ];
    return () => { if (followTimer.current) clearTimeout(followTimer.current); unsubs.forEach(p => void p.then(u => u())); };
  }, [activeId, isViewingCurrent, loadPage, onMessage]);

  const exportCaptions = async () => {
    if (!activeId) return;
    const path = await save({ defaultPath: `LiveCaption-${activeId}.${exportFormat}`, filters: [{ name: "字幕", extensions: [exportFormat] }] });
    if (!path) return;
    try { await exportCaptionSession(path, exportFormat, activeId); onMessage("字幕已导出"); }
    catch (error) { onMessage(String(error)); }
  };

  const startCorrection = async () => {
    if (!activeId) return;
    try {
      await correctCaptionSession(activeId, useGlossary);
      setCorrectionOpen(false);
      setCorrection({ running: true, completed: 0, total: pageView?.total ?? 0 });
    } catch (error) { onMessage(String(error)); }
  };

  const toggleCorrectionPanel = async () => {
    if (correctionOpen) {
      setCorrectionOpen(false);
      return;
    }
    if (isTauri) {
      try {
        setUseGlossary((await getGlossary()).use_in_correction);
      } catch (error) {
        onMessage(String(error));
        return;
      }
    }
    setCorrectionOpen(true);
  };

  const createNewRecord = async () => {
    try { await newCaptionSession(); onMessage("已新建记录，后续转录将写入新记录"); }
    catch (error) { onMessage(String(error)); }
  };

  const correctionBlocked = isViewingCurrent && status.caption_running;
  const headingText = viewingSession
    ? `${formatSessionDateTime(activeSummary?.created_at ?? "")} · ${activeSummary?.entry_count ?? 0} 条`
    : pageView?.total ? `${pageView.total} 条字幕 · 每页 ${SESSION_PAGE_SIZE} 条` : "启动后记录会自动出现在这里";

  return <div className="page">
    <PageHeader eyebrow="LIVE CAPTIONS" title="实时字幕" description={`当前字幕来源：${activeName}。识别结果按顺序交给 LLM 翻译。`}
      action={<StatusDot on={status.caption_state === "running"} text={stateText[status.caption_state]} />} />
    <section className="caption-launcher" data-running={status.caption_running}>
      <div className="caption-pulse">{busy ? <Loader2 className="spin" /> : <Captions />}</div>
      <div className="caption-copy"><span className="overline">{status.caption_running ? "正在监听" : "准备就绪"}</span><h2>{status.caption_running ? `${activeName} 正在识别` : "让声音变成看得懂的字幕"}</h2><p>{status.caption_running ? "partial 会即时更新原文预览，final 结果才会进入翻译和导出。" : `启动后使用 ${activeName}；本地模型会捕获当前默认输出设备。`}</p></div>
      <button className={status.caption_running ? "danger action" : "primary action"} disabled={busy} onClick={onToggle}>{busy ? <Loader2 className="spin" /> : status.caption_running ? <Square /> : <Play />}{actionText}</button>
    </section>
    <CaptionHealth status={status} />
    <div className="section-heading">
      <div><h2>{viewingSession ? "历史记录" : "本次转录"}</h2><p>{headingText}</p></div>
      <div className="export-row">
        {activeSummary?.has_correction && <Segmented value={showCorrected ? "corrected" : "original"} options={[{ label: "原文", value: "original" }, { label: "纠错后", value: "corrected" }]} onChange={value => setShowCorrected(value === "corrected")} />}
        <select value={exportFormat} onChange={e => setExportFormat(e.target.value)}><option value="srt">SRT</option><option value="vtt">WebVTT</option><option value="txt">TXT</option><option value="json">JSON</option></select>
        <button className="ghost" disabled={!pageView?.total} onClick={() => void exportCaptions()}><Download />导出</button>
        {isTauri && <button className="ghost" title="打开字幕目录" onClick={() => void openSessionsDir().catch(error => onMessage(String(error)))}><ExternalLink />目录</button>}
        <button className="ghost" disabled={!activeId || !pageView?.total || correction.running || correctionBlocked} title={correctionBlocked ? "停止实时字幕后可纠错" : undefined} onClick={() => void toggleCorrectionPanel()}><Wand2 />纠错</button>
        {viewingSession
          ? <button className="ghost record-action" onClick={() => onSelectSession(null)}><ArrowLeft />返回本次</button>
          : <button className="ghost record-action" disabled={!isTauri} title="把后续转录写入一条新记录" onClick={() => void createNewRecord()}><Plus />新建记录</button>}
      </div>
    </div>
    {correctionOpen && !correction.running && (
      <div className="correction-bar">
        <span>LLM 将结合上下文修正可能的转写和翻译错误，原始内容会保留。</span>
        <Toggle label="导入术语表" checked={useGlossary} onChange={setUseGlossary} />
        <button className="primary" onClick={() => void startCorrection()}><Wand2 />开始纠错</button>
        <button className="ghost" onClick={() => setCorrectionOpen(false)}>取消</button>
      </div>
    )}
    {correction.running && (
      <div className="correction-bar" data-running="true">
        <Loader2 className="spin" /><span>正在纠错 {correction.completed}/{correction.total || "…"}，可继续浏览其他页面</span>
      </div>
    )}
    <div className="caption-list">
      {pageView?.entries.length ? pageView.entries.map(entry => {
        const corrected = showCorrected && !!entry.corrected_translated_text;
        const isLiveLatest = isViewingCurrent && status.caption_running && entry.index === pageView.total;
        return (
          <motion.article
            key={entry.index}
            data-latest={isLiveLatest}
            data-corrected={corrected}
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: .24, ease: [0.22, 1, 0.36, 1] }}
          >
            <span>{corrected ? entry.corrected_source_text : entry.source_text}</span>
            <p>{(corrected ? entry.corrected_translated_text : entry.translated_text) || entry.error}</p>
            <small title={`#${entry.index} · ${entry.model} · ${entry.latency_ms}ms`}>{formatEntryTime(entry.timestamp)}</small>
          </motion.article>
        );
      }) : <div className="quiet-empty"><FileText /><div><h3>{viewingSession ? "这条记录是空的" : "等待第一句字幕"}</h3><p>这里不会用大面积空框占住你的视线。</p></div></div>}
    </div>
    {pageView && pageView.page_count > 1 && (
      <nav className="pager" aria-label="分页">
        <button disabled={pageView.page <= 1} onClick={() => void loadPage(pageView.page - 1)}><ChevronLeft />上一页</button>
        <label className="pager-jump" title="输入页码，回车跳转">
          <input
            value={pageDraft}
            inputMode="numeric"
            aria-label="页码"
            onChange={e => setPageDraft(e.target.value.replace(/\D/g, ""))}
            onKeyDown={e => { if (e.key === "Enter") { jumpToPage(); e.currentTarget.blur(); } }}
            onBlur={jumpToPage}
          />
          <span>/ {pageView.page_count}</span>
        </label>
        <button disabled={pageView.page >= pageView.page_count} onClick={() => void loadPage(pageView.page + 1)}>下一页<ChevronRight /></button>
      </nav>
    )}
    <SessionTicker sessions={sessions} viewingSession={viewingSession} running={status.caption_running} onSelect={onSelectSession} onMessage={onMessage} />
  </div>;
}

function SessionTicker({ sessions, viewingSession, running, onSelect, onMessage }: {
  sessions: SessionSummary[]; viewingSession: string | null; running: boolean;
  onSelect: (id: string | null) => void; onMessage: (message: string) => void;
}) {
  const [hover, setHover] = useState<{ session: SessionSummary; top: number; left: number } | null>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const list = listRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [sessions.length]);

  useEffect(() => () => { if (closeTimer.current) clearTimeout(closeTimer.current); }, []);

  const openCard = (session: SessionSummary, target: HTMLElement) => {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    const rect = target.getBoundingClientRect();
    setHover({ session, top: Math.min(rect.top, window.innerHeight - 210), left: rect.right + 12 });
  };
  const scheduleClose = () => {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => setHover(null), 160);
  };

  const remove = async (id: string) => {
    if (!window.confirm("删除这条转录记录？此操作不可撤销。")) return;
    if (!isTauri) { onMessage("预览模式无法删除记录"); return; }
    try { await deleteCaptionSession(id); if (viewingSession === id) onSelect(null); }
    catch (error) { onMessage(String(error)); }
  };

  if (!sessions.length) return null;
  const ordered = [...sessions].reverse();
  return <>
    <div className="session-ticker" aria-label="转录记录">
      <div className="ticker-track" ref={listRef}>
        {ordered.map(session => (
          <button
            key={session.id}
            className="ticker-tick"
            data-current={session.is_current}
            data-live={session.is_current && running}
            data-active={session.is_current ? viewingSession === null : viewingSession === session.id}
            style={{ width: Math.min(30, 10 + Math.round(Math.log2(session.entry_count + 1) * 5)) }}
            title={session.id}
            onMouseEnter={e => openCard(session, e.currentTarget)}
            onMouseLeave={scheduleClose}
            onFocus={e => openCard(session, e.currentTarget)}
            onBlur={scheduleClose}
            onClick={() => onSelect(session.is_current ? null : session.id)}
          />
        ))}
      </div>
    </div>
    {hover && createPortal(
      <motion.div
        className="ticker-card"
        style={{ top: hover.top, left: hover.left }}
        initial={{ opacity: 0, x: -6, scale: .97 }}
        animate={{ opacity: 1, x: 0, scale: 1 }}
        transition={{ duration: .16, ease: [0.22, 1, 0.36, 1] }}
        onMouseEnter={() => { if (closeTimer.current) clearTimeout(closeTimer.current); }}
        onMouseLeave={scheduleClose}
      >
        <header>
          <strong>{formatSessionDateTime(hover.session.created_at)}</strong>
          {hover.session.is_current && <i>本次</i>}
        </header>
        <p className="ticker-card-source">{hover.session.first_source || "（还没有转录内容）"}</p>
        {hover.session.first_translation && <p className="ticker-card-translation">{hover.session.first_translation}</p>}
        <footer>
          <small>{hover.session.entry_count} 条{hover.session.has_correction ? " · 已纠错" : ""}</small>
          <button onClick={() => void remove(hover.session.id)}><Trash2 />删除</button>
        </footer>
      </motion.div>,
      document.body,
    )}
  </>;
}

function GlossaryPage({ onMessage }: { onMessage: (message: string) => void }) {
  const demo: Glossary = {
    use_in_translation: false, use_in_correction: true,
    terms: [
      { id: "demo-1", source: "桜井", target: "樱井", note: "常驻嘉宾" },
      { id: "demo-2", source: "ファスト映画", target: "速看电影", note: "" },
    ],
  };
  const [glossary, setGlossary] = useState<Glossary | null>(isTauri ? null : demo);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState({ source: "", target: "", note: "" });

  useEffect(() => {
    if (!isTauri) return;
    void getGlossary().then(setGlossary).catch(error => onMessage(String(error)));
    const unlisten = listen<Glossary>("glossary:changed", e => setGlossary(e.payload));
    return () => { void unlisten.then(u => u()); };
  }, [onMessage]);

  const update = (next: Glossary) => setGlossary(next);
  const updateTerm = (id: string, patch: Partial<{ source: string; target: string; note: string }>) => {
    if (!glossary) return;
    update({ ...glossary, terms: glossary.terms.map(term => term.id === id ? { ...term, ...patch } : term) });
  };
  const addTerm = () => {
    if (!glossary || !draft.source.trim() || !draft.target.trim()) return;
    update({ ...glossary, terms: [...glossary.terms, { id: crypto.randomUUID(), source: draft.source.trim(), target: draft.target.trim(), note: draft.note.trim() }] });
    setDraft({ source: "", target: "", note: "" });
  };
  const persist = async () => {
    if (!glossary || !isTauri) return;
    setSaving(true);
    try { await saveGlossary(glossary); onMessage("术语表已保存"); }
    catch (error) { onMessage(String(error)); }
    finally { setSaving(false); }
  };
  const exportFile = async () => {
    const path = await save({ defaultPath: "livecaption-glossary.json", filters: [{ name: "术语表", extensions: ["json"] }] });
    if (!path) return;
    try { await exportGlossary(path); onMessage("术语表已导出"); }
    catch (error) { onMessage(String(error)); }
  };
  const importFile = async () => {
    const path = await open({ multiple: false, filters: [{ name: "术语表", extensions: ["json"] }] });
    if (!path) return;
    try {
      const result = await importGlossary(path);
      setGlossary(result.glossary);
      onMessage(`导入完成：新增 ${result.added} 条，更新 ${result.updated} 条`);
    } catch (error) { onMessage(String(error)); }
  };

  if (!glossary) return <div className="page"><PageHeader eyebrow="GLOSSARY" title="术语表" description="正在读取术语表…" /></div>;
  return <div className="page">
    <PageHeader eyebrow="GLOSSARY" title="术语表" description="统一人名、作品名等专有名词的译法，可用于实时翻译和字幕纠错；支持导入导出以便多设备同步。"
      action={<button className="primary" onClick={() => void persist()} disabled={saving || !isTauri}>{saving ? <Loader2 className="spin" /> : <Check />}保存术语表</button>} />
    <section className="settings-sheet">
      <SettingRow title="用于实时翻译" description="翻译请求携带术语表，下次启动字幕后生效。"><Toggle label="用于实时翻译" checked={glossary.use_in_translation} onChange={use_in_translation => update({ ...glossary, use_in_translation })} /></SettingRow>
      <SettingRow title="默认用于字幕纠错" description="点击纠错时可临时更改是否导入术语表，这里是默认值。"><Toggle label="默认用于字幕纠错" checked={glossary.use_in_correction} onChange={use_in_correction => update({ ...glossary, use_in_correction })} /></SettingRow>
      <div className="settings-group-title glossary-head">
        <div><h2>词条</h2><p>{glossary.terms.length} 条，按源词去重</p></div>
        <div>
          <button className="ghost" onClick={() => void importFile()} disabled={!isTauri}><Upload />导入</button>
          <button className="ghost" onClick={() => void exportFile()} disabled={!isTauri || !glossary.terms.length}><Download />导出</button>
        </div>
      </div>
      <div className="glossary-add">
        <input placeholder="原文，如 桜井" value={draft.source} onChange={e => setDraft({ ...draft, source: e.target.value })} />
        <input placeholder="译法，如 樱井" value={draft.target} onChange={e => setDraft({ ...draft, target: e.target.value })} />
        <input placeholder="备注（可选）" value={draft.note} onChange={e => setDraft({ ...draft, note: e.target.value })} onKeyDown={e => { if (e.key === "Enter") addTerm(); }} />
        <button className="primary" onClick={addTerm} disabled={!draft.source.trim() || !draft.target.trim()}><Plus />添加</button>
      </div>
      <div className="glossary-list">
        {glossary.terms.length ? glossary.terms.map(term => (
          <div className="glossary-row" key={term.id}>
            <input value={term.source} onChange={e => updateTerm(term.id, { source: e.target.value })} aria-label="原文" />
            <input value={term.target} onChange={e => updateTerm(term.id, { target: e.target.value })} aria-label="译法" />
            <input value={term.note} placeholder="备注" onChange={e => updateTerm(term.id, { note: e.target.value })} aria-label="备注" />
            <button title="删除词条" onClick={() => update({ ...glossary, terms: glossary.terms.filter(item => item.id !== term.id) })}><Trash2 /></button>
          </div>
        )) : <div className="quiet-empty"><BookMarked /><div><h3>还没有词条</h3><p>在上方添加第一组术语，或从其他设备导入。</p></div></div>}
      </div>
    </section>
  </div>;
}

function SettingsPage(props: { anchor?: string; onAnchorHandled: () => void; tab: SettingsTab; setTab: (v: SettingsTab) => void; view: SettingsView; onProfilesChanged: (view: SettingsView) => void; update: (v: AppSettings) => void; onSave: () => void; saving: boolean; testing: boolean; onTest: () => void; logs: RuntimeLogEntry[]; logQuery: string; setLogQuery: (v: string) => void; onClearLogs: () => void; models: ModelInfo[]; status: RuntimeStatus; onSelectSource: (source: CaptionSourceConfig) => Promise<void>; refreshModels: () => void; onMessage: (message: string) => void }) {
  const { tab, setTab, view, update } = props; const s = view.settings;
  const [tocOpen, setTocOpen] = useState(false);
  const [activeChapter, setActiveChapter] = useState("");
  const pendingChapter = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const chapters: Record<SettingsTab, { id: string; title: string }[]> = {
    features: [{ id: "selection", title: "划词翻译" }, { id: "captions", title: "实时字幕" }, { id: "ocr", title: "截图翻译" }],
    resources: [{ id: "asr-models", title: "语音识别 · ASR" }, { id: "ocr-models", title: "文字识别 · OCR" }, { id: "network", title: "网络与下载" }],
    application: [{ id: "translation", title: "翻译服务" }, { id: "diagnostics", title: "日志与诊断" }],
    appearance: [{ id: "theme", title: "主题与动效" }, { id: "overlay", title: "字幕浮窗" }],
  };
  const jumpTo = (id: string) => {
    document.getElementById(`settings-${id}`)?.scrollIntoView({ behavior: s.visual.reduce_motion ? "instant" : "smooth", block: "start" });
    setActiveChapter(id);
  };
  const navigateTo = (next: SettingsTab, chapter: string) => {
    if (next === tab) jumpTo(chapter);
    else { pendingChapter.current = chapter; setTab(next); }
  };
  useEffect(() => {
    localStorage.setItem("settings-category", tab);
    const scroller = contentRef.current?.closest(".content");
    const target = pendingChapter.current;
    pendingChapter.current = null;
    if (target) document.getElementById(`settings-${target}`)?.scrollIntoView({ block: "start" });
    else scroller?.scrollTo({ top: 0 });
    const updateChapter = () => {
      const sections = Array.from(contentRef.current?.querySelectorAll<HTMLElement>(".settings-chapter") ?? []);
      const top = (scroller?.getBoundingClientRect().top ?? 0) + 48;
      const atBottom = scroller && scroller.scrollTop > 0 && scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 3;
      const current = atBottom ? sections.at(-1) : sections.filter(section => section.getBoundingClientRect().top <= top).at(-1) ?? sections[0];
      if (current) setActiveChapter(current.id.replace("settings-", ""));
    };
    updateChapter();
    scroller?.addEventListener("scroll", updateChapter, { passive: true });
    return () => scroller?.removeEventListener("scroll", updateChapter);
  }, [tab]);
  useEffect(() => {
    if (!props.anchor) return;
    document.getElementById(`settings-${props.anchor}`)?.scrollIntoView({ block: "start" });
    setActiveChapter(props.anchor);
    props.onAnchorHandled();
  }, [props.anchor]);
  const [dependencyReport, setDependencyReport] = useState<AsrDependencyReport>();
  const [gpuRuntime, setGpuRuntime] = useState<AsrGpuRuntimeInfo>();
  const [dependencyDialogOpen, setDependencyDialogOpen] = useState(false);
  const [checkingDependencies, setCheckingDependencies] = useState(false);
  const [dependencyError, setDependencyError] = useState("");
  const [testingProxy, setTestingProxy] = useState(false);
  useEffect(() => {
    if (!isTauri) return;
    let unsubscribe: (() => void) | undefined;
    void listen<AsrGpuRuntimeProgressEvent>("gpu-runtime:progress", event => {
      setGpuRuntime(current => current ? { ...current, ...event.payload } : undefined);
      if (["available", "failed", "not_installed"].includes(event.payload.status)) {
        void checkAsrDependencies().then(report => {
          setDependencyReport(report);
          setGpuRuntime(report.gpu_runtime);
        }).catch(() => undefined);
      }
    }).then(stop => { unsubscribe = stop; });
    return () => unsubscribe?.();
  }, []);
  const runDependencyCheck = async () => {
    setDependencyDialogOpen(true); setCheckingDependencies(true); setDependencyError("");
    try { const report = await checkAsrDependencies(); setDependencyReport(report); setGpuRuntime(report.gpu_runtime); }
    catch (error) { setDependencyError(String(error)); }
    finally { setCheckingDependencies(false); }
  };
  const startGpuRuntimeDownload = async () => {
    try {
      await downloadAsrGpuRuntime();
      setGpuRuntime(current => current ? { ...current, status: "downloading", error: undefined } : undefined);
      setDependencyError("");
    } catch (error) { setDependencyError(String(error)); }
  };
  const stopGpuRuntimeDownload = async () => {
    try { await cancelAsrGpuRuntime(); } catch (error) { setDependencyError(String(error)); }
  };
  const testProxyConnection = async () => {
    setTestingProxy(true);
    try { props.onMessage(await testDownloadProxy(s.downloads.proxy_url)); }
    catch (error) { props.onMessage(String(error)); }
    finally { setTestingProxy(false); }
  };
  const tabs: { id: SettingsTab; label: string }[] = [{ id: "appearance", label: "外观" }, { id: "features", label: "功能" }, { id: "resources", label: "资源" }, { id: "application", label: "应用" }];
  const filteredLogs = props.logs.filter(l => `${l.level} ${l.message}`.toLowerCase().includes(props.logQuery.toLowerCase()));
  return <div className="page settings-page" ref={contentRef}>
    <PageHeader eyebrow="PREFERENCES" title="设置" description="让翻译，按照你的习惯运行。" action={<button className="primary" onClick={props.onSave} disabled={props.saving}>{props.saving ? <Loader2 className="spin" /> : <Check />}保存设置</button>} />
    <div className="settings-tabs" role="tablist" aria-label="设置分类">
      {tabs.map(t => <button id={`tab-${t.id}`} role="tab" aria-selected={tab === t.id} aria-controls="settings-panel" data-active={tab === t.id} key={t.id} onClick={() => setTab(t.id)} onKeyDown={event => {
        const direction = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
        if (!direction && event.key !== "Home" && event.key !== "End") return;
        event.preventDefault();
        const next = tabs[event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (tabs.findIndex(item => item.id === t.id) + direction + tabs.length) % tabs.length];
        setTab(next.id); document.getElementById(`tab-${next.id}`)?.focus();
      }} tabIndex={tab === t.id ? 0 : -1}>
        {tab === t.id && <motion.span className="tab-thumb" layoutId="settings-tab" transition={{ type: "spring", stiffness: 480, damping: 40 }} />}<span>{t.label}</span>
      </button>)}
    </div>
    <div className="settings-layout" data-toc-open={tocOpen}>
    <div id="settings-panel" role="tabpanel" aria-labelledby={`tab-${tab}`}>
{tab === "features" && <><section className="settings-chapter" id="settings-selection" aria-labelledby="heading-selection">
      <header className="chapter-heading"><div><h2 id="heading-selection">划词翻译</h2><p>选中文字，就近获取翻译。</p></div></header>

        <SettingRow title="启用划词翻译" description="在其他应用中选中文字后提供轻量翻译工具。"><Toggle label="启用划词翻译" checked={s.selection.enabled} onChange={enabled => update({ ...s, selection: { ...s.selection, enabled } })} /></SettingRow>
        <SettingRow title="划词触发方式" description="自动模式只在明确拖选结束后显示按钮；双击选词不会触发。"><Segmented value={s.selection.trigger_mode} options={[{ label: "快捷键", value: "hotkey" }, { label: "自动出现", value: "automatic" }]} onChange={trigger_mode => update({ ...s, selection: { ...s.selection, trigger_mode } })} /></SettingRow>
        {s.selection.trigger_mode === "hotkey" && <SettingRow title="划词触发快捷键" description="至少包含一个修饰键；Ctrl+Win+L 保留给 Windows 实时字幕。"><HotkeyRecorder value={s.selection.hotkey} onChange={hotkey => update({ ...s, selection: { ...s.selection, hotkey } })} /></SettingRow>}
        <SettingRow title="剪贴板回退" description="无法通过 UI Automation 读取选区时，允许短暂读取剪贴板。"><Toggle label="启用回退" checked={s.selection.clipboard_fallback_enabled} onChange={clipboard_fallback_enabled => update({ ...s, selection: { ...s.selection, clipboard_fallback_enabled } })} /></SettingRow>

    </section><section className="settings-chapter" id="settings-captions" aria-labelledby="heading-captions">
      <header className="chapter-heading"><div><h2 id="heading-captions">实时字幕</h2><p>选择字幕来源，调整音频识别和翻译行为。</p></div><button className="ghost chapter-link" onClick={() => navigateTo("resources", "asr-models")}>管理 ASR 资源<ArrowRight /></button></header>

        <div className="settings-group"><div className="settings-group-title"><div><h3>字幕来源</h3><p>选择后立即生效；运行中会先检查新来源，成功后开启全新会话。</p></div>{props.status.caption_state === "switching" && <span><Loader2 className="spin" />正在切换字幕来源</span>}</div>
          <div className="source-grid">
            <SourceCard title="Windows Live Caption" description="读取 Windows 系统字幕；不占用 GPU。" state="系统组件" selected={s.captions.source.type === "windows_live_caption"} onClick={() => void props.onSelectSource({ type: "windows_live_caption" })} />
            {props.models.filter(model => model.kind === "asr").map(model => <SourceCard key={model.id} title={model.display_name} description={model.recommended ? "日语优化、速度优先，推荐默认使用。" : "复杂音频精度优先，显存占用更高。"} state={modelStatusText(model.status)} badge={model.recommended ? "推荐" : undefined} selected={s.captions.source.type === "local_asr" && s.captions.source.model_id === model.id} onClick={() => void props.onSelectSource({ type: "local_asr", model_id: model.id, device: "cuda", compute_type: "int8_float16", vad_profile: s.captions.audio_mode, channel_mode: s.captions.source.type === "local_asr" ? s.captions.source.channel_mode : "auto", channel_switch_sensitivity: s.captions.source.type === "local_asr" ? s.captions.source.channel_switch_sensitivity : "standard", suppress_non_speech_segments: s.captions.source.type === "local_asr" ? s.captions.source.suppress_non_speech_segments : true })} />)}
          </div>
        </div>
        <SettingRow title="启用实时字幕" description="启动后读取当前字幕来源，并按下方规则组织上下文和音频。"><Toggle label="启用实时字幕" checked={s.captions.enabled} onChange={enabled => update({ ...s, captions: { ...s.captions, enabled } })} /></SettingRow>
        <SettingRow title="翻译上下文" description="上下文来自已接受的 final 原文，不受上一条 LLM 成败影响。"><Segmented value={String(s.captions.context_segments)} options={[0, 1, 2, 4].map(value => ({ label: `${value} 条`, value: String(value) }))} onChange={value => update({ ...s, captions: { ...s.captions, context_segments: Number(value) } })} /></SettingRow>
        {s.captions.source.type === "local_asr" && <>
          <SettingRow title="音频模式" description="普通模式保持原有单声道流程；ASMR 才会启用双声道人声锁定。"><Segmented value={s.captions.audio_mode} options={[{ label: "普通", value: "normal" }, { label: "ASMR", value: "asmr" }]} onChange={audio_mode => update({ ...s, captions: { ...s.captions, audio_mode, source: { type: "local_asr", model_id: s.captions.source.type === "local_asr" ? s.captions.source.model_id : "kotoba-whisper-v2.0-faster", device: "cuda", compute_type: "int8_float16", vad_profile: audio_mode, channel_mode: s.captions.source.type === "local_asr" ? s.captions.source.channel_mode : "auto", channel_switch_sensitivity: s.captions.source.type === "local_asr" ? s.captions.source.channel_switch_sensitivity : "standard", suppress_non_speech_segments: s.captions.source.type === "local_asr" ? s.captions.source.suppress_non_speech_segments : true } } })} /></SettingRow>
          {s.captions.audio_mode === "asmr" && <div className="nested-settings">
            <SettingRow title="ASMR 人声声道" description="智能锁定不会在句间停顿时换边；只有另一侧持续出现可信人声才会切换。"><Segmented value={s.captions.source.channel_mode === "mono" ? "auto" : s.captions.source.channel_mode} options={[{ label: "智能锁定", value: "auto" }, { label: "保持混合", value: "mix" }, { label: "仅左", value: "left" }, { label: "仅右", value: "right" }]} onChange={channel_mode => { const source = s.captions.source; if (source.type === "local_asr") update({ ...s, captions: { ...s.captions, source: { ...source, channel_mode } } }); }} /></SettingRow>
            {s.captions.source.channel_mode === "auto" && <>
              <SettingRow title="声道切换灵敏度" description="稳健模式需要更长的人声证据；灵敏模式更快，但更容易被复杂噪音影响。"><Segmented value={s.captions.source.channel_switch_sensitivity} options={[{ label: "稳健", value: "stable" }, { label: "标准", value: "standard" }, { label: "灵敏", value: "responsive" }]} onChange={channel_switch_sensitivity => { const source = s.captions.source; if (source.type === "local_asr") update({ ...s, captions: { ...s.captions, source: { ...source, channel_switch_sensitivity } } }); }} /></SettingRow>
              <SettingRow title="疑似非语音字幕抑制" description="在人声证据不足时不把摩擦、敲击等短片段送入 ASR。"><Toggle label="疑似非语音字幕抑制" checked={s.captions.source.suppress_non_speech_segments} onChange={suppress_non_speech_segments => { const source = s.captions.source; if (source.type === "local_asr") update({ ...s, captions: { ...s.captions, source: { ...source, suppress_non_speech_segments } } }); }} /></SettingRow>
              <SettingRow title="当前人声声道" description={asmrChannelStateText[props.status.asmr_channel_state]}><button className="ghost" disabled={!props.status.caption_running} onClick={() => void resetAsrChannelRouting().catch(error => props.onMessage(String(error)))}><RotateCcw />重新检测</button></SettingRow>

            </>}
          </div>}
        </>}
        {s.captions.source.type === "windows_live_caption" && <>
          <SettingRow title="自动启动 Live Captions" description="开始翻译时自动打开 Windows 系统字幕。"><Toggle label="自动启动" checked={s.captions.auto_launch} onChange={auto_launch => update({ ...s, captions: { ...s.captions, auto_launch } })} /></SettingRow>
          <div className="three-cols"><Field label="轮询间隔 (ms)"><input type="number" min="80" value={s.captions.poll_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, poll_milliseconds: Number(e.target.value) } })} /></Field><Field label="稳定时间 (ms)"><input type="number" min="100" value={s.captions.stable_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, stable_milliseconds: Number(e.target.value) } })} /></Field><Field label="最长分段 (ms)"><input type="number" min="500" value={s.captions.max_duration_milliseconds} onChange={e => update({ ...s, captions: { ...s.captions, max_duration_milliseconds: Number(e.target.value) } })} /></Field></div>
        </>}
    </section><section className="settings-chapter" id="settings-ocr" aria-labelledby="heading-ocr">
      <header className="chapter-heading"><div><h2 id="heading-ocr">截图翻译</h2><p>框选屏幕文字，识别后按文字块翻译。</p></div><button className="ghost chapter-link" onClick={() => navigateTo("resources", "ocr-models")}>管理 OCR 资源<ArrowRight /></button></header>
      <SettingRow title="启用截图翻译" description="从工具栏或托盘启动框选，识别文字后逐块翻译。"><Toggle label="启用截图翻译" checked={s.ocr.enabled} onChange={enabled => update({ ...s, ocr: { ...s.ocr, enabled } })} /></SettingRow>
      <div className="two-cols"><Field label="默认原文语言" hint="用于翻译；OCR 模型支持中、英、日文识别。"><select value={s.ocr.source_language} onChange={e => update({ ...s, ocr: { ...s.ocr, source_language: e.target.value } })}><option value="auto">自动检测</option>{["中文", "English", "日本語"].map(language => <option key={language}>{language}</option>)}</select></Field>
      <Field label="默认译文语言" hint="留空时跟随应用中的翻译服务设置。"><input maxLength={40} placeholder={`跟随翻译服务 · ${s.llm.target_language}`} value={s.ocr.target_language} onChange={e => update({ ...s, ocr: { ...s.ocr, target_language: e.target.value } })} /></Field></div>
      <SettingRow title="原文位置" description="设置结果窗口的默认布局；也可以在结果窗口临时调整。"><Segmented value={s.ocr.source_placement} options={[{ label: "每块译文下方", value: "inline" }, { label: "底部显示选中块", value: "bottom" }]} onChange={source_placement => update({ ...s, ocr: { ...s.ocr, source_placement } })} /></SettingRow>
    </section></>}{tab === "resources" && <><section className="settings-chapter" id="settings-asr-models" aria-labelledby="heading-asr-models">
      <header className="chapter-heading"><div><h2 id="heading-asr-models">语音识别 · ASR</h2><p>管理语音模型与 GPU 运行环境，供实时字幕使用。</p></div><button className="ghost chapter-link" onClick={() => navigateTo("features", "captions")}>字幕设置<ArrowRight /></button></header>
              <SettingRow title="本地 ASR 运行环境" description="检查 Worker、NVIDIA 驱动、CUDA/cuDNN 实际加载状态和 CTranslate2 GPU 可用性；旧 Worker 会自动改用模型推理验证。">
          <button className="ghost" onClick={() => void runDependencyCheck()} disabled={checkingDependencies}>{checkingDependencies ? <Loader2 className="spin" /> : <Activity />}检查依赖</button>
        </SettingRow>
        <ModelManager models={props.models.filter(model => model.kind === "asr")} activeModel={props.status.active_model} modelDirectory={view.model_directory} refresh={props.refreshModels} message={props.onMessage} />

    </section><section className="settings-chapter" id="settings-ocr-models" aria-labelledby="heading-ocr-models">
      <header className="chapter-heading"><div><h2 id="heading-ocr-models">文字识别 · OCR</h2><p>PP-OCRv5 Mobile · 本地 CPU 识别中、英、日文，闲置后自动释放。</p></div><button className="ghost chapter-link" onClick={() => navigateTo("features", "ocr")}>截图设置<ArrowRight /></button></header>
              <ModelManager title="OCR 模型管理" models={props.models.filter(model => model.kind === "ocr")} modelDirectory={view.model_directory} refresh={props.refreshModels} message={props.onMessage} />

    </section><section className="settings-chapter" id="settings-network" aria-labelledby="heading-network">
      <header className="chapter-heading"><div><h2 id="heading-network">网络与下载</h2><p>仅用于本应用的模型和运行组件下载，不影响其他软件。</p></div></header>
                <Field label="模型镜像" hint="官方 Hugging Face 连接失败后使用；两条线路执行相同 SHA-256 校验。"><input value={s.captions.model_mirror_url} onChange={e => update({ ...s, captions: { ...s.captions, model_mirror_url: e.target.value } })} /></Field>
          <SettingRow title="应用专用代理" description="关闭后立即清空代理地址；下载请求不会退回系统代理，避免配置意外泄漏到其他应用。"><Toggle label="启用下载代理" checked={s.downloads.proxy_enabled} onChange={proxy_enabled => update({ ...s, downloads: { proxy_enabled, proxy_url: proxy_enabled ? s.downloads.proxy_url : "" } })} /></SettingRow>
          {s.downloads.proxy_enabled && <Field label="代理地址" hint="支持 HTTP/HTTPS 代理，例如 http://127.0.0.1:10809。保存后仅本应用的模型和运行库下载使用。">
            <div className="proxy-input-actions"><input inputMode="url" placeholder="http://127.0.0.1:10809" value={s.downloads.proxy_url} onChange={e => update({ ...s, downloads: { ...s.downloads, proxy_url: e.target.value } })} /><button className="ghost" onClick={() => void testProxyConnection()} disabled={testingProxy || !s.downloads.proxy_url.trim()}>{testingProxy ? <Loader2 className="spin" /> : <Activity />}测试连接</button></div>
          </Field>}

    </section></>}{tab === "application" && <><section className="settings-chapter" id="settings-translation" aria-labelledby="heading-translation">
      <header className="chapter-heading"><div><h2 id="heading-translation">翻译服务</h2><p>划词、字幕和截图翻译共用连接配置。</p></div></header>

      <LlmProfiles view={view} onChanged={props.onProfilesChanged} onMessage={props.onMessage} onTest={props.onTest} testing={props.testing} />

    </section><section className="settings-chapter" id="settings-diagnostics" aria-labelledby="heading-diagnostics">
      <header className="chapter-heading"><div><h2 id="heading-diagnostics">日志与诊断</h2><p>检查运行状态，定位识别或翻译中的问题。</p></div></header>

        <div className="log-toolbar"><label><Search /><input value={props.logQuery} onChange={e => props.setLogQuery(e.target.value)} placeholder="搜索日志" /></label><button className="ghost" onClick={() => void openLogsDir()}><ExternalLink />目录</button><button className="ghost" onClick={props.onClearLogs}><Eraser />清空</button></div>
        <div className="log-view">{filteredLogs.length ? [...filteredLogs].reverse().map((l, i) => <div className="log-line" key={`${l.timestamp}-${i}`}><span data-level={l.level}>{l.level.toUpperCase()}</span><time>{new Date(l.timestamp).toLocaleTimeString()}</time><p>{l.message}</p></div>) : <EmptyState icon={<FileText />} title="日志很安静" text="运行状态和错误信息会显示在这里。" />}</div>

    </section></>}{tab === "appearance" && <><section className="settings-chapter" id="settings-theme" aria-labelledby="heading-theme">
      <header className="chapter-heading"><div><h2 id="heading-theme">主题与动效</h2><p>调整整个应用的视觉体验。</p></div></header>
              <SettingRow title="主题" description="跟随系统或固定使用浅色、深色主题。"><Segmented value={s.visual.theme} options={[{ label: "系统", value: "system" }, { label: "浅色", value: "light" }, { label: "深色", value: "dark" }]} onChange={theme => update({ ...s, visual: { ...s.visual, theme } })} /></SettingRow>
        <SettingRow title="高斯模糊" description="为字幕和划词浮窗启用半透明模糊背景。"><Toggle label="高斯模糊" checked={s.visual.blur_enabled} onChange={blur_enabled => update({ ...s, visual: { ...s.visual, blur_enabled } })} /></SettingRow>
        <SettingRow title="减少动画" description="关闭页面切换和浮窗展开动画。"><Toggle label="减少动画" checked={s.visual.reduce_motion} onChange={reduce_motion => update({ ...s, visual: { ...s.visual, reduce_motion } })} /></SettingRow>
    </section><section className="settings-chapter" id="settings-overlay" aria-labelledby="heading-overlay">
      <header className="chapter-heading"><div><h2 id="heading-overlay">字幕浮窗</h2><p>设置字幕的显示方式与拖动习惯。</p></div></header>
      <SettingRow title="字幕浮窗拖动" description="选择最顺手的移动方式；拖栏模式可避免误拖。"><Segmented value={s.overlay.drag_mode} options={[{ label: "Alt + 拖动", value: "alt" }, { label: "整体拖动", value: "anywhere" }, { label: "顶部拖栏", value: "handle" }]} onChange={drag_mode => update({ ...s, overlay: { ...s.overlay, drag_mode } })} /></SettingRow>
        <Field label={`字幕字号 · ${s.overlay.font_size}px`}><Slider min={14} max={34} value={s.overlay.font_size} onChange={font_size => update({ ...s, overlay: { ...s.overlay, font_size } })} /></Field>
        <SettingRow title="字幕背景" description="透明模式只保留字幕文字和悬浮控制区。"><Toggle label="透明字幕背景" checked={s.overlay.transparent} onChange={transparent => update({ ...s, overlay: { ...s.overlay, transparent } })} /></SettingRow>
        <Field label={`字幕浮窗透明度 · ${Math.round(s.overlay.opacity * 100)}%`} hint="同时调整文字与背景的整体透明度"><Slider min={0.2} max={1} step={0.05} value={s.overlay.opacity} onChange={opacity => update({ ...s, overlay: { ...s.overlay, opacity } })} /></Field>
        <SettingRow title="字幕颜色" description="也可以直接在字幕浮窗右下角随时更换。"><label className="color-control"><input type="color" value={s.overlay.caption_color} onChange={e => update({ ...s, overlay: { ...s.overlay, caption_color: e.target.value } })} /><span>{s.overlay.caption_color.toUpperCase()}</span></label></SettingRow>
    </section></>}<footer className="settings-footer"><span>修改完成后，保存以应用设置。</span><button className="primary" onClick={props.onSave} disabled={props.saving}>{props.saving ? <Loader2 className="spin" /> : <Check />}保存设置</button></footer>
    </div>
    <aside className="settings-toc" aria-label="本页目录" data-open={tocOpen}>
      <button className="toc-handle" aria-label={tocOpen ? "收起目录" : "展开本页目录"} aria-expanded={tocOpen} aria-controls="settings-toc-panel" onClick={() => setTocOpen(!tocOpen)}>{tocOpen ? <ChevronRight /> : <ChevronLeft />}<span>目录</span></button>
      <div className="toc-panel" id="settings-toc-panel" inert={!tocOpen} aria-hidden={!tocOpen}>
        <div className="toc-heading"><span>本页目录</span></div>
        <nav>{chapters[tab].map((chapter, index) => <button key={chapter.id} aria-current={activeChapter === chapter.id ? "location" : undefined} onClick={() => jumpTo(chapter.id)}><span>{String(index + 1).padStart(2, "0")}</span>{chapter.title}</button>)}</nav><small>点击章节跳转 · 手动收起</small>
      </div>
    </aside>
    </div>
    {createPortal(
      <AnimatePresence>
        {dependencyDialogOpen && <DependencyDialog report={dependencyReport} gpuRuntime={gpuRuntime} checking={checkingDependencies} error={dependencyError} onClose={() => setDependencyDialogOpen(false)} onCheck={() => void runDependencyCheck()} onDownload={() => void startGpuRuntimeDownload()} onCancel={() => void stopGpuRuntimeDownload()} />}
      </AnimatePresence>,
      document.body,
    )}
  </div>;
}

function DependencyDialog({ report, gpuRuntime, checking, error, onClose, onCheck, onDownload, onCancel }: { report?: AsrDependencyReport; gpuRuntime?: AsrGpuRuntimeInfo; checking: boolean; error: string; onClose: () => void; onCheck: () => void; onDownload: () => void; onCancel: () => void }) {
  const runtime = gpuRuntime ?? report?.gpu_runtime;
  const runtimeProgress = runtime?.total_bytes ? Math.min(100, runtime.downloaded_bytes / runtime.total_bytes * 100) : 0;
  const runtimeBusy = runtime ? ["downloading", "verifying", "installing"].includes(runtime.status) : false;
  const phaseText = runtime?.status === "verifying" ? "正在校验完整性" : runtime?.status === "installing" ? "正在安装缺失组件" : "正在下载缺失组件";
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);
  return (
    <motion.div className="dependency-backdrop" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: .16 }} onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <motion.section
        className="dependency-dialog" role="dialog" aria-modal="true" aria-labelledby="dependency-title"
        initial={{ opacity: 0, scale: .94, y: 18 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: .97, y: 8, transition: { duration: .14 } }}
        transition={{ type: "spring", stiffness: 380, damping: 32 }}
      >
        <header><div><span>LOCAL ASR</span><h2 id="dependency-title">运行环境检查</h2><p>{checking ? "正在调用 ASR Worker 验证 GPU 运行环境…" : runtimeBusy ? `${phaseText}，完成后会自动重新验证。` : report?.ready ? "本地 ASR 所需依赖已完整安装。" : "已根据当前环境拆分检测 CUDA 计算库与 cuDNN，只需补齐缺失组件。"}</p></div><button className="dialog-close" aria-label="关闭" onClick={onClose}><X /></button></header>
        {checking && !report ? <div className="dependency-loading"><Loader2 className="spin" /><span>正在检查，请稍候</span></div> : error ? <div className="dependency-error">{error}</div> : <div className="dependency-list">
          {report?.dependencies.map(item => <article key={item.id} data-installed={item.installed}>
            <div className="dependency-state">{item.installed ? <Check /> : <X />}</div>
            <div className="dependency-copy"><strong>{item.name}</strong><p>{item.detail}</p>{item.detected_path && <code title={item.detected_path}>{item.detected_path}</code>}</div>
            {!item.installed && <button className="ghost" onClick={() => void openAsrDependencyUrl(item.id)}><ExternalLink />打开官网</button>}
          </article>)}
          {runtime && <article className="dependency-runtime" data-installed={runtime.status === "available"}>
            <div className="dependency-state">{runtime.status === "available" ? <Check /> : <Download />}</div>
            <div className="dependency-copy"><strong>GPU Runtime 组件</strong><p>逐项复用系统环境或应用缓存；仅从 PyPI 下载实际缺失的固定版本，并在安装前校验完整性。</p>
              <div className="runtime-components">{runtime.components.map(component => <div key={component.id} data-status={component.status}><span>{component.status === "available" ? <Check /> : <Download />}<strong>{component.name}</strong></span><small>{runtimeComponentSourceText(component.source)}{component.status === "missing" ? ` · 需下载 ${formatBytes(component.download_size_bytes)}` : ""}</small></div>)}</div>
              {runtimeBusy && <div className="download-progress" role="progressbar" aria-label={phaseText} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(runtimeProgress)} data-phase={runtime.status}><i style={{ width: `${runtimeProgress}%` }} /><small>{phaseText} · {runtime.total_bytes ? `${runtimeProgress.toFixed(1)}% · ` : ""}{formatBytes(runtime.downloaded_bytes)}</small></div>}{runtime.error && <small className="model-error">{runtime.error}</small>}</div>
            {runtimeBusy ? <button className="ghost" onClick={onCancel}><Square />取消</button> : runtime.required_download_bytes > 0 ? <button className="primary" onClick={onDownload}><Download />下载缺失组件 · {formatBytes(runtime.required_download_bytes)}</button> : <span className="dependency-ready"><Check />无需下载</span>}
          </article>}
        </div>}
        <footer><button className="ghost" onClick={onClose}>关闭</button><button className="primary" onClick={onCheck} disabled={checking}>{checking ? <Loader2 className="spin" /> : <RefreshCw />}重新检查</button></footer>
      </motion.section>
    </motion.div>
  );
}

function SourceCard({ title, description, state, badge, selected, onClick }: { title: string; description: string; state: string; badge?: string; selected: boolean; onClick: () => void }) {
  return <button className="source-card" data-selected={selected} onClick={onClick}>
    {selected && <span className="source-check"><Check /></span>}
    <span className="source-card-top"><strong>{title}</strong>{badge && <i>{badge}</i>}</span>
    <p>{description}</p><small>{state}</small>
  </button>;
}

function modelStatusText(status: ModelInfo["status"]) {
  const labels: Record<ModelInfo["status"], string> = { not_installed: "未安装", downloading: "下载中", verifying: "校验中", available: "可用", loading: "加载中", active: "活动中", corrupt: "损坏", incompatible: "不兼容", failed: "失败" };
  return labels[status];
}

function modelStatusTone(status: ModelInfo["status"]): "accent" | "success" | "danger" | undefined {
  if (status === "downloading" || status === "verifying" || status === "loading") return "accent";
  if (status === "available" || status === "active") return "success";
  if (status === "corrupt" || status === "incompatible" || status === "failed") return "danger";
  return undefined;
}

function runtimeComponentSourceText(source: AsrGpuRuntimeInfo["components"][number]["source"]) {
  return { system: "使用系统环境", cache: "使用应用缓存", missing: "当前缺失" }[source];
}

function formatBytes(bytes: number) {
  if (!bytes) return "0 MB";
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(2)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
}

function ModelManager({ models, activeModel, modelDirectory, refresh, message, title = "字幕模型管理" }: { title?: string; models: ModelInfo[]; activeModel?: string; modelDirectory: string; refresh: () => void; message: (text: string) => void }) {
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
    <div className="settings-group-title"><div><h3>{title}</h3><p title={modelDirectory}>按需下载和加载，可随时校验、测试或删除。</p></div><div><button className="ghost" onClick={() => void openModelsDir()}><ExternalLink />模型目录</button><button className="ghost" onClick={refresh}><RefreshCw />重新检查</button></div></div>
    {models.map(model => {
      const progress = model.expected_size_bytes ? Math.min(100, model.downloaded_bytes / model.expected_size_bytes * 100) : 0;
      const installed = model.status === "available" || model.status === "active";
      const busyLabel = busy[model.id];
      return <article className="model-row" key={model.id} data-status={model.status}>
        <div className="model-copy"><div><strong>{model.display_name}</strong><span className="badge" data-tone={modelStatusTone(model.status)}>{modelStatusText(model.status)}</span></div><p>{model.repository} · {model.license} · {formatBytes(model.expected_size_bytes)}</p>{model.status === "downloading" && <div className="download-progress"><i style={{ width: `${progress}%` }} /><small>{progress.toFixed(1)}% · {formatBytes(model.downloaded_bytes)}</small></div>}{model.error && <small className="model-error">{model.error}</small>}</div>
        <div className="model-actions">
          {busyLabel ? <button className="ghost" disabled><Loader2 className="spin" />正在{busyLabel}</button> : model.status === "downloading" ? <button className="ghost" onClick={() => void action(model.id, "取消下载", () => cancelModelDownload(model.id))}>暂停/取消</button> : !installed ? <button className="primary" onClick={() => void action(model.id, model.status === "not_installed" ? "下载" : "重试", () => downloadModel(model.id))}><Download />{model.status === "not_installed" ? "下载" : "重试"}</button> : <><button className="ghost" onClick={() => void action(model.id, "校验", () => verifyModel(model.id))}>校验</button><button className="ghost" disabled={activeModel === model.id} onClick={() => void action(model.id, "测试", () => testModel(model.id))}>测试</button><button className="ghost danger-text" disabled={activeModel === model.id} onClick={() => { if (confirm(`删除 ${model.display_name}？`)) void action(model.id, "删除", () => deleteModel(model.id)); }}><Trash2 /></button></>}
        </div>
      </article>;
    })}
  </div>;
}

function Logo() { return <div className="logo" title="LiveCaption"><svg viewBox="0 0 48 48" aria-hidden="true"><path d="M8 9h25a7 7 0 0 1 7 7v12a7 7 0 0 1-7 7H21l-8 7v-7H8a6 6 0 0 1-6-6V15a6 6 0 0 1 6-6Z" /><path d="M16 18h15M16 25h10" /></svg></div>; }
function RailButton({ active, label, onClick, children }: { active: boolean; label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button className="rail-button" data-active={active} aria-current={active ? "page" : undefined} title={label} onClick={onClick}>
      {active && <motion.span className="rail-active" layoutId="rail-active" transition={{ type: "spring", stiffness: 480, damping: 40 }} />}
      {children}<span>{label}</span>
    </button>
  );
}
function StatusDot({ on, text }: { on: boolean; text: string }) { return <div className="status-dot" data-on={on}><i />{text}</div>; }
function SettingRow({ title, description, children }: { title: string; description: string; children: React.ReactNode }) { return <div className="setting-row"><div><h3>{title}</h3><p>{description}</p></div>{children}</div>; }
function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) { return <label className="field"><span>{label}{hint && <small>{hint}</small>}</span>{children}</label>; }
function ResultBlock({ result }: { result: TranslationResult }) { return <article className="result-block"><div><span>{result.source_text}</span><button title="复制译文" onClick={() => void navigator.clipboard.writeText(result.translated_text)}><Copy /></button></div><p>{result.translated_text}</p><small>{result.model} · {result.latency_ms}ms</small></article>; }
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

function mergeProfileView(current: SettingsView, next: SettingsView): SettingsView {
  return { ...current, api_key_configured: next.api_key_configured, settings: { ...current.settings, llm: next.settings.llm, llm_profiles: next.settings.llm_profiles } };
}
