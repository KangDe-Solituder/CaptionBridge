import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { ArrowRight, Check, Copy, Languages, Loader2, Minus, RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { getSettings, translateSelection } from "../lib/api";
import type { AppSettings, SelectionReadyEvent, SettingsView, TranslationResult } from "../lib/contracts";
import { emptyResult, previewSettings, useTheme } from "../lib/ui-shared";
import { currentWindow, isTauri } from "../lib/window";

const springPop = { type: "spring", stiffness: 520, damping: 32 } as const;

export function ToolbarWindow() {
  const [selection, setSelection] = useState<SelectionReadyEvent>();
  const [pending, setPending] = useState(false);
  const [result, setResult] = useState<TranslationResult>();
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const [showOriginal, setShowOriginal] = useState(true);
  const [settings, setSettings] = useState<AppSettings>();
  const [session, setSession] = useState(0);
  const copiedTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!isTauri) {
      setSettings(previewSettings);
      return;
    }
    void getSettings().then(v => setSettings(v.settings));
    const ps = [
      listen("selection:pending", async () => {
        setSelection(undefined); setPending(true); setResult(undefined); setExpanded(false); setCopied(false); setSession(s => s + 1);
        await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44));
      }),
      listen("selection:cancelled", async () => { setPending(false); await currentWindow.hide(); }),
      listen<SelectionReadyEvent>("selection:ready", async e => {
        setSelection(e.payload); setPending(false); setResult(undefined); setExpanded(false); setCopied(false); setSession(s => s + 1);
        await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44));
      }),
      listen<string>("translation:delta", e => setResult(v => ({ ...(v ?? emptyResult), translated_text: `${v?.translated_text ?? ""}${e.payload}` }))),
      listen<SettingsView>("settings:changed", e => { setSettings(e.payload.settings); setResult(undefined); }),
    ];
    return () => { if (copiedTimer.current) clearTimeout(copiedTimer.current); ps.forEach(p => void p.then(u => u())); };
  }, []);

  useTheme(settings);

  const translate = async () => {
    if (!selection || !settings) return;
    setExpanded(true); setBusy(true); setResult(undefined);
    try { await currentWindow.setFocusable(true); await currentWindow.setSize(new LogicalSize(520, 340)); await currentWindow.show(); await currentWindow.setFocus(); } catch { /* translation must still proceed */ }
    try { setResult(await translateSelection({ mode: "selection", source_text: selection.text, source_language: "auto", target_language: settings.llm.target_language })); }
    catch (error) { setResult({ ...emptyResult, source_text: selection.text, error: String(error) }); }
    finally { setBusy(false); }
  };
  const copyText = async (text: string) => {
    await navigator.clipboard.writeText(text); setCopied(true);
    if (copiedTimer.current) clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopied(false), 1000);
  };
  const collapse = async () => {
    setExpanded(false); setResult(undefined);
    try { await currentWindow.setFocusable(false); await currentWindow.setSize(new LogicalSize(246, 44)); } catch { /* keep the toolbar usable */ }
  };

  const copyIcon = (
    <motion.span key={copied ? "check" : "copy"} initial={{ scale: .5, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} transition={springPop} style={{ display: "grid", placeItems: "center" }}>
      {copied ? <Check /> : <Copy />}
    </motion.span>
  );

  return (
    <MotionConfig reducedMotion={settings?.visual.reduce_motion ? "always" : "never"}>
      <AnimatePresence mode="wait" initial={false}>
        {!expanded ? (
          <motion.main
            key={`toolbar-${session}`}
            className="selection-toolbar floating"
            initial={{ opacity: 0, scale: .9, y: 8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: .96, transition: { duration: .12 } }}
            transition={springPop}
          >
            <button className="toolbar-action primary-action" disabled={pending || !selection} onClick={() => void translate()}>{pending ? <Loader2 className="spin" /> : <Languages />}<span>翻译</span></button>
            <span className="toolbar-divider" />
            <button className="toolbar-action" disabled={pending || !selection} onClick={() => selection && void copyText(selection.text)}>{copyIcon}<span>{copied ? "已复制" : "复制"}</span></button>
            <span className="toolbar-divider" />
            <button className="toolbar-action close-action" onClick={() => void currentWindow.hide()}><X /><span>关闭</span></button>
          </motion.main>
        ) : (
          <motion.main
            key="result"
            className="selection-result floating"
            initial={{ opacity: 0, scale: .97 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: .98, transition: { duration: .1 } }}
            transition={{ duration: .18, ease: [0.22, 1, 0.36, 1] }}
          >
            <header className="result-titlebar" onMouseDown={e => { if (e.button === 0 && !(e.target as HTMLElement).closest("button")) void currentWindow.startDragging(); }}>
              <div><Languages /><strong>翻译</strong></div>
              <nav>
                <button title="收起" onClick={e => { e.stopPropagation(); void collapse(); }}><Minus /></button>
                <button title="关闭" onClick={e => { e.stopPropagation(); void currentWindow.hide(); }}><X /></button>
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
          </motion.main>
        )}
      </AnimatePresence>
    </MotionConfig>
  );
}
