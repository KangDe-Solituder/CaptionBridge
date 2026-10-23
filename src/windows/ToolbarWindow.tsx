import { enabledToolbarButtons, toolbarButtons, type ToolbarButton } from "../lib/toolbar-modules";
import { Fragment } from "react";
import { startOcrCapture } from "./OcrWindow";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { MotionConfig, motion } from "motion/react";
import { ArrowRight, Check, Copy, Languages, Loader2, Minus, RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { getSettings, hideSelectionToolbar, translateSelection } from "../lib/api";
import type { AppSettings, ToolbarSnapshot, SettingsView, TranslationResult } from "../lib/contracts";
import { emptyResult, previewSettings, useTheme } from "../lib/ui-shared";
import { currentWindow, isTauri } from "../lib/window";

const springPop = { type: "spring", stiffness: 520, damping: 32 } as const;

async function dismissCurrentSelectionToolbar() {
  try {
    await hideSelectionToolbar();
  } catch {
    try { await currentWindow.setFocusable(false); }
    finally { await currentWindow.hide(); }
  }
}

export function ToolbarWindow() {
  const [selection, setSelection] = useState<ToolbarSnapshot>();
  const [result, setResult] = useState<TranslationResult>();
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const [showOriginal, setShowOriginal] = useState(true);
  const [settings, setSettings] = useState<AppSettings>();
  const copiedTimer = useRef<number | undefined>(undefined);
  const revision = useRef(-1);
  const activeRequest = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!isTauri) {
      setSettings(previewSettings);
      return;
    }
    void getSettings().then(v => setSettings(v.settings));
    let alive = true;
    const apply = (snapshot: ToolbarSnapshot) => {
      if (!alive || snapshot.revision <= revision.current) return;
      revision.current = snapshot.revision;
      setSelection(snapshot.text.trim() ? snapshot : undefined);
      setExpanded(snapshot.mode === "expanded");
      if (snapshot.mode !== "expanded") {
        activeRequest.current = undefined;
        setResult(undefined); setBusy(false); setCopied(false);
      }
    };
    const ps = [
      listen<ToolbarSnapshot>("toolbar:state", e => apply(e.payload)).then(async unlisten => {
        // Subscribe before reading: a cold webview must not miss its initial state.
        try { apply(await invoke<ToolbarSnapshot>("get_toolbar_snapshot")); } catch { /* the next event refreshes state */ }
        return unlisten;
      }),
      listen<{ stream_id: string; text: string }>("toolbar:translation-delta", e => {
        if (e.payload.stream_id === activeRequest.current) setResult(v => ({ ...(v ?? emptyResult), translated_text: `${v?.translated_text ?? ""}${e.payload.text}` }));
      }),
      listen<SettingsView>("settings:changed", e => { setSettings(e.payload.settings); setResult(undefined); }),
      listen<SettingsView>("llm:profiles-changed", e => { setSettings(e.payload.settings); }),
    ];
    return () => { alive = false; activeRequest.current = undefined; if (copiedTimer.current) clearTimeout(copiedTimer.current); ps.forEach(p => void p.then(u => u())); };
  }, []);

  useTheme(settings);

  const translate = async () => {
    if (!selection || !settings || busy) return;
    const ticket = crypto.randomUUID();
    activeRequest.current = ticket;
    setBusy(true); setResult(undefined);
    try {
      await invoke("expand_toolbar");
      if (activeRequest.current !== ticket) return;
      setExpanded(true);
      const translated = await translateSelection({ mode: "selection", source_text: selection.text, source_language: "auto", target_language: settings.llm.target_language }, ticket);
      if (activeRequest.current === ticket) setResult(translated);
    } catch (error) {
      if (activeRequest.current === ticket) setResult({ ...emptyResult, source_text: selection.text, error: String(error) });
    } finally {
      if (activeRequest.current === ticket) { activeRequest.current = undefined; setBusy(false); }
    }
  };
  const copyText = async (text: string) => {
    await navigator.clipboard.writeText(text); setCopied(true);
    if (copiedTimer.current) clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopied(false), 1000);
    await dismiss();
  };
  const dismiss = async () => {
    activeRequest.current = undefined;
    setSelection(undefined); setExpanded(false); setResult(undefined); setBusy(false);
    await dismissCurrentSelectionToolbar();
  };

  const runButton = async (id: ToolbarButton) => {
    if (id === "translate") { await translate(); return; }
    if (id === "copy") { if (selection) await copyText(selection.text); return; }
    if (id === "ocr") {
      await startOcrCapture().catch(async error => { await invoke("open_ocr_resources"); await emitTo("main", "ocr:error", String(error)); });
      return;
    }
    setBusy(true);
    try { await invoke("start_captions_from_toolbar"); }
    catch (error) { await emitTo("main", "runtime:error", String(error)); }
    finally { setBusy(false); }
  };

  const copyIcon = (
    <motion.span key={copied ? "check" : "copy"} initial={{ scale: .5, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} transition={springPop} style={{ display: "grid", placeItems: "center" }}>
      {copied ? <Check /> : <Copy />}
    </motion.span>
  );

  return (
    <MotionConfig reducedMotion={settings?.visual.reduce_motion ? "always" : "never"}>
      {!expanded ? (
        <main
          key="toolbar"
          className="selection-toolbar floating"
        >
          {enabledToolbarButtons(settings ?? previewSettings).map((id, index) => {
            const { icon: Icon, title } = toolbarButtons[id];
            const needsSelection = id === "translate" || id === "copy";
            return <Fragment key={id}>
              {index > 0 && <span className="toolbar-divider" />}
              <button className={`toolbar-action ${id === "translate" ? "primary-action" : ""}`} title={needsSelection && !selection ? `选中文字后可${title}` : id === "captions" ? "启动字幕浮窗" : title} disabled={(needsSelection && !selection) || busy} onClick={() => void runButton(id)}>{id === "copy" ? copyIcon : <Icon />}<span>{title}</span></button>
            </Fragment>;
          })}
        </main>
      ) : (
        <main
          key="result"
          className="selection-result floating"
        >
          <header className="result-titlebar" onMouseDown={e => { if (e.button === 0 && !(e.target as HTMLElement).closest("button")) void currentWindow.startDragging(); }}>
            <div><Languages /><strong>翻译</strong></div>
            <nav>
              <button title="收起" onClick={e => { e.stopPropagation(); void dismiss(); }}><Minus /></button>
              <button title="关闭" onClick={e => { e.stopPropagation(); void dismiss(); }}><X /></button>
            </nav>
          </header>
          <div className="language-row">
            <span>自动检测</span><ArrowRight /><strong>{settings?.llm.target_language}</strong>
            <button onClick={() => setShowOriginal(v => !v)}>{showOriginal ? "隐藏原文" : "显示原文"}</button>
          </div>
          <section className="result-content">
            {showOriginal && <p className="source-text">{selection?.text}</p>}
            {busy && !result?.translated_text
              ? <div className="result-loading"><Loader2 className="spin" /><span>正在连接模型…</span></div>
              : <p className="translated-text" data-streaming={busy}>{result?.translated_text || result?.error}</p>}
          </section>
          <footer className="result-footer">
            <small>{result?.model ? `${result.model} · 首字 ${result.first_token_ms}ms · 完成 ${result.latency_ms}ms` : ""}</small>
            <div>
              <button disabled={busy} onClick={() => void translate()}><RotateCcw />重新生成</button>
              <button disabled={busy || !result?.translated_text} onClick={() => result && void copyText(result.translated_text)}>{copyIcon}{copied ? "已复制" : "复制"}</button>
            </div>
          </footer>
        </main>
      )}
    </MotionConfig>
  );
}
