import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { ChevronDown, ChevronUp, GripHorizontal, Loader2, Palette, RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { getRuntimeStatus, getSettings, refreshCaption, saveOverlayPosition, saveSettings, setCaptionRunning } from "../lib/api";
import type { AppSettings, CaptionSourceHealth, CaptionTranslatedEvent, SettingsView } from "../lib/contracts";
import { useTheme, previewSettings } from "../lib/ui-shared";
import { currentWindow, isTauri } from "../lib/window";

export function OverlayWindow() {
  const [items, setItems] = useState<CaptionTranslatedEvent[]>([]);
  const [currentSource, setCurrentSource] = useState("");
  const [health, setHealth] = useState<CaptionSourceHealth>("unknown");
  const [expanded, setExpanded] = useState(false);
  const [settings, setSettings] = useState<AppSettings>();
  const [controls, setControls] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const feedRef = useRef<HTMLDivElement>(null);
  const positionTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!isTauri) {
      setSettings(previewSettings);
      return;
    }
    void getSettings().then(v => setSettings(v.settings));
    void getRuntimeStatus().then(v => setHealth(v.source_health));
    const ps = [
      listen<string>("caption:source", e => setCurrentSource(e.payload)),
      listen<CaptionTranslatedEvent>("caption:translated", e => setItems(v => [...v, e.payload].slice(-12))),
      listen<CaptionSourceHealth>("caption:health-changed", e => setHealth(e.payload)),
      listen<SettingsView>("settings:changed", e => setSettings(e.payload.settings)),
    ];
    const moved = currentWindow.onMoved(e => {
      if (positionTimer.current) clearTimeout(positionTimer.current);
      positionTimer.current = window.setTimeout(() => void saveOverlayPosition(e.payload.x, e.payload.y), 180);
    });
    return () => { if (positionTimer.current) clearTimeout(positionTimer.current); ps.forEach(p => void p.then(u => u())); void moved.then(u => u()); };
  }, []);

  useEffect(() => {
    const feed = feedRef.current;
    if (feed) requestAnimationFrame(() => feed.scrollTo({ top: feed.scrollHeight, behavior: "smooth" }));
  }, [items, expanded]);

  useTheme(settings);

  const toggleExpanded = async () => {
    const next = !expanded;
    setExpanded(next);
    await currentWindow.setSize(new LogicalSize(settings?.overlay.width ?? 760, next ? 320 : 170));
  };
  const persistOverlay = async (patch: Partial<AppSettings["overlay"]>) => {
    if (!settings) return;
    const next = { ...settings, overlay: { ...settings.overlay, ...patch } };
    setSettings(next);
    try { await saveSettings(next); } catch { /* keep controls responsive */ }
  };
  const refresh = async () => { if (refreshing) return; setRefreshing(true); try { await refreshCaption(); } finally { setRefreshing(false); } };
  const drag = (e: React.MouseEvent, fromHandle = false) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button,input")) return;
    const mode = settings?.overlay.drag_mode ?? "alt";
    if ((mode === "alt" && e.altKey) || mode === "anywhere" || (mode === "handle" && fromHandle)) void currentWindow.startDragging();
  };
  const dragHint = settings?.overlay.drag_mode === "anywhere" ? "拖动任意位置" : settings?.overlay.drag_mode === "handle" ? "拖动此处" : "ALT + 拖动";

  return (
    <MotionConfig reducedMotion={settings?.visual.reduce_motion ? "always" : "never"}>
      <main
        className="overlay-window floating"
        data-expanded={expanded}
        data-transparent={settings?.overlay.transparent}
        style={{ fontSize: settings?.overlay.font_size, opacity: settings?.overlay.opacity, "--caption-color": settings?.overlay.caption_color } as React.CSSProperties}
        onMouseDown={e => drag(e)}
      >
        <header className="overlay-bar" onMouseDown={e => drag(e, true)}>
          <span><i data-health={health} />实时翻译</span>
          <div>
            <small><GripHorizontal />{dragHint}</small>
            <button title="刷新字幕连接" disabled={refreshing} onClick={() => void refresh()}>{refreshing ? <Loader2 className="spin" /> : <RefreshCw />}</button>
            <button title={expanded ? "收起字幕记录" : "展开字幕记录"} onClick={() => void toggleExpanded()}>{expanded ? <ChevronUp /> : <ChevronDown />}</button>
            <button className="overlay-close" title="结束实时字幕" onClick={() => void setCaptionRunning(false)}><X /></button>
          </div>
        </header>
        <div className="caption-feed" ref={feedRef}>
          {items.map(item => (
            <motion.section
              key={item.segment.id}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: .24, ease: [0.22, 1, 0.36, 1] }}
            >
              <span>{item.segment.source_text}</span>
              <p>{item.result.translated_text || item.result.error}</p>
            </motion.section>
          ))}
          {!items.length && <p className="muted waiting-caption">{currentSource ? "正在整理第一句翻译…" : "等待实时字幕…"}</p>}
        </div>
        {currentSource && <aside className="recognition-dock"><span><i />实时识别</span><p title={currentSource}>{currentSource}</p></aside>}
        <div className="overlay-quick">
          <button data-active={controls} title="字幕外观" onClick={() => setControls(v => !v)}><Palette /></button>
          <AnimatePresence>
            {controls && (
              <motion.div
                className="overlay-popover"
                initial={{ opacity: 0, y: 6, scale: .95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 4, scale: .97 }}
                transition={{ type: "spring", stiffness: 460, damping: 30 }}
              >
                <div><span>背景</span><button className="mini-toggle" data-active={settings?.overlay.transparent} onClick={() => void persistOverlay({ transparent: !settings?.overlay.transparent })}>{settings?.overlay.transparent ? "透明" : "毛玻璃"}</button></div>
                <label><span>字幕颜色</span><input type="color" value={settings?.overlay.caption_color ?? "#ffffff"} onChange={e => void persistOverlay({ caption_color: e.target.value })} /></label>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </main>
    </MotionConfig>
  );
}
