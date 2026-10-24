import { Moon, Sun, Monitor, Grip, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSettings } from "../lib/contracts";
import { currentWindow, isTauri } from "../lib/window";

export function useSidebarDock() {
  const [mode, setMode] = useState<"auto" | "fixed" | "floating">(() => {
    const value = localStorage.getItem("workbench-sidebar");
    return value === "fixed" || value === "floating" ? value : "auto";
  });
  const [wide, setWide] = useState(window.innerWidth >= 1440);
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    let alive = true;
    const update = () => {
      setWide(window.innerWidth >= 1440);
      if (isTauri) void currentWindow.isMaximized().then(value => { if (alive) setMaximized(value); }).catch(() => {});
      else setMaximized(!!document.fullscreenElement);
    };
    update();
    window.addEventListener("resize", update);
    document.addEventListener("fullscreenchange", update);
    return () => { alive = false; window.removeEventListener("resize", update); document.removeEventListener("fullscreenchange", update); };
  }, []);
  const fixed = mode === "fixed" || (mode === "auto" && (wide || maximized));
  const changeMode = (next: typeof mode) => { setMode(next); localStorage.setItem("workbench-sidebar", next); };
  return { fixed, mode, toggle: () => changeMode(fixed ? "floating" : "fixed"), reset: () => changeMode("auto") };
}

function useSystemDark() {
  const [dark, setDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const update = () => setDark(media.matches);
    update(); media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return dark;
}

export function SidebarThemeControl({ theme, onChange, disabled }: { theme: AppSettings["visual"]["theme"]; onChange: (theme: AppSettings["visual"]["theme"]) => void; disabled: boolean }) {
  const systemDark = useSystemDark();
  const dark = theme === "dark" || (theme === "system" && systemDark);
  const label = theme === "system" ? "跟随系统" : dark ? "深色模式" : "浅色模式";
  const action = dark ? "切换浅色模式" : "切换深色模式";
  return <div className="rail-theme">
    <button className="rail-theme-toggle" disabled={disabled} aria-label={action} title={`${label} · ${action}（自动保存）`} onClick={() => onChange(dark ? "light" : "dark")}>
      {dark ? <Moon /> : <Sun />}<span className="rail-label">{label}</span>
    </button>
    <button className="rail-theme-system" disabled={disabled || theme === "system"} aria-label="主题跟随系统" title="主题跟随系统（自动保存）" aria-pressed={theme === "system"} onClick={() => onChange("system")}><Monitor /></button>
  </div>;
}

export function AppearanceChoices({ settings: s, onChange }: { settings: AppSettings; onChange: (s: AppSettings) => void }) {
  const systemDark = useSystemDark();
  const dark = s.visual.theme === "dark" || (s.visual.theme === "system" && systemDark);
  return <>
    <div className="appearance-mode"><div><h3>明暗模式</h3><p>{s.visual.theme === "system" ? "正在跟随系统" : dark ? "深色" : "明亮"}</p></div><div><button className="ghost" onClick={() => onChange({ ...s, visual: { ...s.visual, theme: dark ? "light" : "dark" } })}>{dark ? <Sun /> : <Moon />}{dark ? "切换明亮" : "切换深色"}</button><button className="ghost" disabled={s.visual.theme === "system"} onClick={() => onChange({ ...s, visual: { ...s.visual, theme: "system" } })}>跟随系统</button></div></div>
    <div className="accent-choices" aria-label="配色风格">
      {([{ id: "purple", title: "经典紫", text: "现有风格 · 紫蓝点缀" }, { id: "green", title: "荧光绿", text: "清晰醒目的局部强调" }, { id: "blue", title: "晴空蓝", text: "轻盈柔和的蓝色点缀" }] as const).map(choice => <button key={choice.id} type="button" data-palette={choice.id} aria-pressed={(s.visual.accent ?? "purple") === choice.id} onClick={() => onChange({ ...s, visual: { ...s.visual, accent: choice.id } })}><span className="accent-swatch"><i /><i /><i /></span><strong>{choice.title}</strong><small>{choice.text}</small></button>)}
    </div>
    <p className="appearance-note">风格只调整按钮、选中态与局部强调色，保留现有窗口底色。</p>
  </>;
}

export function SubtitlePreview({ settings: s }: { settings: AppSettings }) {
  return <figure className="subtitle-preview" aria-label="字幕浮窗样式预览">
    <div className="subtitle-preview-stage">
      <div className="subtitle-preview-window" data-transparent={s.overlay.transparent} style={{ opacity: s.overlay.opacity }}>
        <div className="subtitle-preview-header"><span><i />实时翻译</span><span><Grip />{s.overlay.drag_mode === "alt" ? "Alt + 拖动" : s.overlay.drag_mode === "handle" ? "顶部拖栏" : "整体拖动"}</span></div>
        <p style={{ fontSize: s.overlay.font_size, color: s.overlay.caption_color }}>如果有问题，请在评论里告诉我</p>
        <div className="subtitle-preview-source"><span>質問があれば、コメントで。</span><Settings /></div>
      </div>
    </div>
    <figcaption>设置内预览 · 保存后应用到字幕浮窗</figcaption>
  </figure>;
}
