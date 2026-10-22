import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, ChevronDown, ChevronUp, Copy, Grip, Loader2, MoreHorizontal, RefreshCw, ScanText, Settings, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { currentWindow, isTauri } from "../lib/window";
import "./ocr.css";

type Frame = { id: number; image: string; width: number; height: number };
type Rect = [number, number, number, number];
type Block = { id: number; text: string; translation: string; confidence: number; bounds: Rect; error?: string };
type ResultView = { session: number; status: string; blocks: Block[]; error?: string; load_ms: number; recognize_ms: number; source_language: string; target_language: string };
const empty: ResultView = { session: 0, status: "idle", blocks: [], load_ms: 0, recognize_ms: 0, source_language: "auto", target_language: "中文" };
const number = (id: number) => String(id).padStart(2, "0");
export const startOcrCapture = () => invoke<void>("start_ocr_capture");
const cancelOcr = () => invoke<void>("cancel_ocr");
const openResources = () => invoke<void>("open_ocr_resources");

export function OcrCaptureWindow() {
  const [frame, setFrame] = useState<Frame>();
  const [rect, setRect] = useState<Rect>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const origin = useRef<[number, number] | undefined>(undefined);
  const surface = useRef<HTMLDivElement>(null);
  const submitting = useRef(false);
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    const refresh = () => void invoke<Frame | null>("get_ocr_capture").then(value => {
      if (!alive || !value) return;
      setFrame(value); setRect(undefined); setError(""); setBusy(false); submitting.current = false;
    }).catch(e => alive && setError(String(e)));
    const unsubscribe = listen("ocr:capture", refresh);
    const clear = listen("ocr:clear-capture", () => { if (alive) { setFrame(undefined); setRect(undefined); } });
    refresh();
    return () => { alive = false; void unsubscribe.then(f => f()); void clear.then(f => f()); };
  }, []);
  const submit = useCallback(async () => {
    if (!frame || !rect || submitting.current || !surface.current) return;
    const bounds = surface.current.getBoundingClientRect();
    const sx = frame.width / bounds.width, sy = frame.height / bounds.height;
    const x = Math.max(0, Math.round(rect[0] * sx)), y = Math.max(0, Math.round(rect[1] * sy));
    const crop = [x, y, Math.min(frame.width - x, Math.round(rect[2] * sx)), Math.min(frame.height - y, Math.round(rect[3] * sy))];
    if (crop[2] < 8 || crop[3] < 8) { setError("选区太小，请重新拖动框选"); return; }
    submitting.current = true; setBusy(true);
    try { await invoke("recognize_ocr", { id: frame.id, rect: crop, sourceLanguage: localStorage.getItem("ocr-source-language"), targetLanguage: localStorage.getItem("ocr-target-language") }); setFrame(undefined); }
    catch (e) { setError(String(e)); }
    finally { submitting.current = false; setBusy(false); }
  }, [frame, rect]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); void cancelOcr(); }
      if (e.key === "Enter") { e.preventDefault(); void submit(); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [submit]);
  const point = (e: React.PointerEvent): [number, number] => {
    const b = e.currentTarget.getBoundingClientRect();
    return [Math.max(0, Math.min(b.width, e.clientX - b.left)), Math.max(0, Math.min(b.height, e.clientY - b.top))];
  };
  return <div className="ocr-capture" ref={surface} onContextMenu={e => { e.preventDefault(); void cancelOcr(); }}
    onPointerDown={e => { if (e.button !== 0 || busy || !frame) return; origin.current = point(e); setRect(undefined); setError(""); e.currentTarget.setPointerCapture(e.pointerId); }}
    onPointerMove={e => { if (!origin.current) return; const [x, y] = point(e), [ox, oy] = origin.current; setRect([Math.min(x, ox), Math.min(y, oy), Math.abs(x - ox), Math.abs(y - oy)]); }}
    onPointerUp={() => { origin.current = undefined; }} onPointerCancel={() => { origin.current = undefined; }}>
    {frame && <img src={`data:image/png;base64,${frame.image}`} draggable={false} alt="待框选的屏幕截图" />}
    {rect ? <div className="ocr-crop" style={{ left: rect[0], top: rect[1], width: rect[2], height: rect[3] }} /> : <div className="ocr-dim" />}
    <div className="ocr-capture-hint" onPointerDown={e => e.stopPropagation()}>
      <ScanText size={18} /><span>{error || (frame ? "拖动框选文字 · Enter 识别 · Esc / 右键取消" : "正在准备截图…")}</span>
      <button disabled={!rect || busy} onClick={() => void submit()}>{busy ? <Loader2 className="spin" size={17} /> : <Check size={17} />}识别并翻译</button>
      <button aria-label="取消截图" onClick={() => void cancelOcr()}><X size={18} /></button>
    </div>
  </div>;
}

// Browser preview data never reaches the OCR worker or translation provider.
const preview: ResultView = { ...empty, session: 1, status: "ready", recognize_ms: 428, blocks: [
  { id: 1, text: "Enjoy a quiet morning in Kyoto.", translation: "在京都，享受宁静的早晨。", confidence: .99, bounds: [0, 0, 400, 30] },
  { id: 2, text: "Open daily\n9:00 – 18:00", translation: "每日开放\n9:00 – 18:00", confidence: .98, bounds: [0, 80, 300, 70] },
  { id: 3, text: "Please keep your ticket.", translation: "请保管好您的门票。", confidence: .97, bounds: [0, 180, 320, 30] },
] };
type Menu = { x: number; y: number; block: Block; sourceOnly: boolean };
const languages = ["中文", "English", "日本語", "한국어", "Français", "Deutsch", "Español"];
export function OcrResultWindow() {
  const [view, setView] = useState<ResultView>(isTauri ? empty : preview);
  const [selected, setSelected] = useState(1);
  const [placement, setPlacement] = useState(() => localStorage.getItem("ocr-source-placement") === "bottom" ? "bottom" : "inline");
  const [expanded, setExpanded] = useState(false);
  const [settings, setSettings] = useState(false);
  const [source, setSource] = useState("auto"), [target, setTarget] = useState("中文");
  const [menu, setMenu] = useState<Menu>();
  const [notice, setNotice] = useState("");
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const session = useRef(0);
  const notify = (text: string) => { setNotice(text); clearTimeout(noticeTimer.current); noticeTimer.current = setTimeout(() => setNotice(""), 3500); };
  useEffect(() => {
    if (!isTauri) return;
    let alive = true, eventReceived = false;
    const apply = (value: ResultView) => {
      if (!alive || value.session < session.current) return;
      if (value.session !== session.current) { setSelected(value.blocks[0]?.id ?? 1); setMenu(undefined); setSettings(false); setSource(value.source_language); setTarget(value.target_language); session.current = value.session; }
      setView(value);
    };
    const unlisten = listen<ResultView>("ocr:result", e => { eventReceived = true; apply(e.payload); });
    void unlisten.then(() => invoke<ResultView>("get_ocr_result")).then(v => { if (!eventReceived) apply(v); }).catch(e => alive && notify(String(e)));
    return () => { alive = false; clearTimeout(noticeTimer.current); void unlisten.then(f => f()); };
  }, []);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") { if (menu) setMenu(undefined); else if (settings) setSettings(false); else void cancelOcr(); }
    };
    const dismiss = () => setMenu(undefined);
    window.addEventListener("keydown", key); window.addEventListener("resize", dismiss);
    return () => { window.removeEventListener("keydown", key); window.removeEventListener("resize", dismiss); };
  }, [menu, settings]);
  const showMenu = (e: React.MouseEvent, block: Block, sourceOnly = false, anchor = false) => {
    e.preventDefault(); e.stopPropagation(); setSelected(block.id); setSettings(false);
    const r = e.currentTarget.getBoundingClientRect();
    const x = anchor ? r.right : e.clientX, y = anchor ? r.bottom : e.clientY;
    const height = sourceOnly && placement === "bottom" && !anchor ? 94 : 150;
    setMenu({ block, sourceOnly: sourceOnly && placement !== "inline" && !anchor,
      x: Math.max(8, Math.min(x, window.innerWidth - 220)), y: Math.max(8, y + height > window.innerHeight - 8 ? y - height : y) });
  };
  const copy = async (text: string) => {
    setMenu(undefined);
    try { await navigator.clipboard.writeText(text); notify("已复制"); } catch (e) { notify(`复制失败：${String(e)}`); }
  };
  const retry = async () => {
    localStorage.setItem("ocr-source-language", source); localStorage.setItem("ocr-target-language", target);
    if (!isTauri) { setSettings(false); return; }
    try { await invoke("retry_ocr_translation", { sourceLanguage: source, targetLanguage: target }); } catch (e) { notify(String(e)); }
  };
  const current = view.blocks.find(b => b.id === selected) ?? view.blocks[0];
  const working = view.status === "recognizing" || view.status === "translating";
  return <main className="ocr-result" onPointerDown={e => { if (!(e.target as HTMLElement).closest(".ocr-context")) setMenu(undefined); }}>
    <header className="ocr-header" onPointerDown={e => { if (e.button === 0 && !(e.target as HTMLElement).closest("button")) void currentWindow.startDragging(); }}>
      <span className="ocr-title"><ScanText size={17} />截图翻译</span><span className="ocr-drag"><Grip size={14} />拖动</span>
      <button title="重新截图" aria-label="重新截图" onClick={() => void startOcrCapture().catch(e => notify(String(e)))}><ScanText size={17} /></button>
      <button title="重新翻译" aria-label="重新翻译" disabled={!view.blocks.length || working} onClick={() => void retry()}><RefreshCw size={17} /></button>
      <button title="关闭" aria-label="关闭截图翻译" onClick={() => void cancelOcr()}><X size={18} /></button>
    </header>
    <section className="ocr-content" onScroll={() => setMenu(undefined)}>
      {view.blocks.length > 0 && <p className="ocr-count">{view.blocks.length} 个文字块 · 按阅读顺序{view.status === "translating" && <span><Loader2 size={12} className="spin" />翻译中</span>}</p>}
      {view.blocks.map(block => <article key={block.id} className="ocr-block" data-selected={selected === block.id} onClick={() => setSelected(block.id)}>
        <span className="ocr-index">{number(block.id)}</span><div className="ocr-block-content">
          <div className="ocr-translation" tabIndex={0} onContextMenu={e => showMenu(e, block)}>{block.translation || <span className="ocr-placeholder">{block.error ? "翻译失败" : "等待翻译…"}</span>}</div>
          {block.error && <p className="ocr-error">{block.error}</p>}
          {placement === "inline" && <div className="ocr-original" tabIndex={0} onContextMenu={e => showMenu(e, block, true)}>{block.text}</div>}
          {block.confidence < .8 && <small className="ocr-confidence">识别把握较低，建议核对原文</small>}
        </div><button className="ocr-block-more" aria-label={`文字块 ${number(block.id)} 复制菜单`} onClick={e => showMenu(e, block, false, true)}><MoreHorizontal size={18} /></button>
      </article>)}
      {!view.blocks.length && <div className="ocr-empty">
        {view.status === "recognizing" ? <><Loader2 size={25} className="spin" /><strong>正在识别截图</strong><span>首次使用需要加载本地模型</span></> : <><ScanText size={27} /><strong>{view.status === "empty" ? "没有识别到文字" : view.status === "error" ? "识别未完成" : "框选屏幕上的文字"}</strong><span>{view.error || "请尝试较清晰的文字区域，避免选区过小。"}</span>
          <div><button onClick={() => void startOcrCapture().catch(e => notify(String(e)))}>重新截图</button><button onClick={() => void openResources()}>管理 OCR 资源</button></div></>}
      </div>}
    </section>
    <footer className="ocr-footer">
      {placement === "bottom" && current ? <><span className="ocr-source-label">原文 {number(current.id)}</span><div className="ocr-bottom-source" data-expanded={expanded} tabIndex={0} onContextMenu={e => showMenu(e, current, true)}>{current.text}</div><button aria-label={expanded ? "收起原文" : "展开原文"} onClick={() => setExpanded(!expanded)}>{expanded ? <ChevronDown size={16} /> : <ChevronUp size={16} />}</button></> : <span className="ocr-footer-meta">{working ? "处理中…" : `${view.blocks.length ? `识别 ${view.recognize_ms}ms · ` : ""}${view.target_language || "中文"}`}<small>右键文字可分别复制</small></span>}
      <button className="ocr-settings-button" aria-label="截图翻译设置" title="设置" onClick={() => { setSettings(!settings); setMenu(undefined); }}><Settings size={18} /></button>
    </footer>
    {settings && <><button className="ocr-backdrop" aria-label="关闭设置" onClick={() => setSettings(false)} /><section className="ocr-settings">
      <strong>截图翻译设置</strong>
      <label>原文位置<select value={placement} onChange={e => { setPlacement(e.target.value); localStorage.setItem("ocr-source-placement", e.target.value); }}><option value="inline">每块译文下方</option><option value="bottom">底部显示选中块</option></select></label>
      <div className="ocr-language-row"><label>原文语言<select value={source} onChange={e => setSource(e.target.value)}><option value="auto">自动检测</option>{languages.map(l => <option key={l}>{l}</option>)}</select></label><span>→</span><label>译文语言<select value={target} onChange={e => setTarget(e.target.value)}>{!languages.includes(target) && <option>{target}</option>}{languages.map(l => <option key={l}>{l}</option>)}</select></label></div>
      <p>语言设置用于翻译。OCR 当前使用中、英、日文通用模型。</p>
      <button className="ocr-apply" disabled={!view.blocks.length || view.status === "recognizing"} onClick={() => void retry()}>应用并重新翻译</button><button onClick={() => void openResources()}>管理 OCR 资源</button>
    </section></>}
    {menu && <div className="ocr-context" role="menu" style={{ left: menu.x, top: menu.y }} onContextMenu={e => e.preventDefault()}>
      <small>文字块 {number(menu.block.id)} · 复制</small>
      {!menu.sourceOnly && <button role="menuitem" disabled={!menu.block.translation || !!menu.block.error} onClick={() => void copy(menu.block.translation)}><Copy size={14} />复制译文</button>}
      <button role="menuitem" onClick={() => void copy(menu.block.text)}><Copy size={14} />复制原文</button>
      {!menu.sourceOnly && <button role="menuitem" disabled={!menu.block.translation || !!menu.block.error} onClick={() => void copy(`${menu.block.text}\n\n${menu.block.translation}`)}>复制原文和译文</button>}
    </div>}
    {notice && <div className="ocr-notice" role="status">{notice}</div>}
  </main>;
}
