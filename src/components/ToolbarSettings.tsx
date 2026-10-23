import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowDown, ArrowUp, GripVertical, ShieldCheck } from "lucide-react";
import { Reorder, useDragControls } from "motion/react";
import { useEffect, useId, useRef, useState } from "react";
import type { AppSettings, MouseAction, MouseCapabilities, OcrSettings, ToolbarModule } from "../lib/contracts";
import { enabledToolbarButtons, toolbarButtons, toolbarModules } from "../lib/toolbar-modules";
import { isTauri } from "../lib/window";

export function ToolbarSettings({ settings, onChange }: { settings: AppSettings; onChange: (value: AppSettings) => void }) {
  const value = settings.ocr;
  const [previewSelection, setPreviewSelection] = useState(false);
  const [mouse, setMouse] = useState<MouseCapabilities>({ detected: false, buttons: 0, hook_ready: false });
  const [selected, setSelected] = useState(1);
  const clip = useId().replace(/:/g, "");
  const first = useRef<HTMLSelectElement>(null);
  const second = useRef<HTMLSelectElement>(null);
  useEffect(() => {
    if (!isTauri) { setMouse({ detected: true, buttons: 5, hook_ready: true }); return; }
    let alive = true;
    const refresh = () => invoke<MouseCapabilities>("get_mouse_capabilities").then(next => { if (alive) setMouse(next); }).catch(() => { if (alive) setMouse({ detected: false, buttons: 0, hook_ready: false, error: "鼠标检测暂不可用" }); });
    const unlisten = listen<MouseCapabilities>("mouse:capabilities", e => { if (alive) setMouse(e.payload); });
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => { alive = false; clearInterval(timer); void unlisten.then(stop => stop()); };
  }, []);
  const supported = mouse.detected && mouse.buttons >= 4 && mouse.hook_ready;
  const configurable = supported && value.mouse_shortcuts;
  const patch = (next: Partial<OcrSettings>) => onChange({ ...settings, ocr: { ...value, ...next } });
  const enabledCount = settings.toolbar_order.filter(module => settings[module].enabled).length;
  const toggleModule = (module: ToolbarModule) => {
    if (settings[module].enabled && enabledCount === 1) return;
    onChange({ ...settings, [module]: { ...settings[module], enabled: !settings[module].enabled } });
  };
  const moveModule = (index: number, direction: number) => {
    const order = [...settings.toolbar_order];
    [order[index], order[index + direction]] = [order[index + direction], order[index]];
    onChange({ ...settings, toolbar_order: order });
  };
  const status = mouse.error ?? (!mouse.detected ? "未检测到鼠标 · 接入带侧键的鼠标后可配置" : mouse.buttons < 4 ? "当前鼠标没有可用侧键" : !mouse.hook_ready ? "侧键监听不可用，请重新启动应用" : `已检测到 ${mouse.buttons} 键鼠标`);
  return <div className="ocr-toolbar-settings">
    <div className="setting-row"><div><strong>常驻工具栏</strong><p>保持在桌面右下角、任务栏上方。完成操作后回到停靠位置。</p></div><label className="ocr-entry-switch"><input type="checkbox" aria-label="常驻工具栏" checked={value.resident_toolbar} onChange={e => patch({ resident_toolbar: e.target.checked })} /><span /></label></div>
    <div className="setting-row"><div><strong>鼠标侧键唤出</strong><p>{status}</p></div><label className="ocr-entry-switch"><input type="checkbox" aria-label="鼠标侧键唤出" disabled={!supported} checked={value.mouse_shortcuts} onChange={e => patch({ mouse_shortcuts: e.target.checked })} /><span /></label></div>
    <div className="ocr-mouse-config">
      <figure className="ocr-mouse-figure">
        <div className="ocr-mouse-drawing">
          <svg viewBox="0 0 360 210" role="img" aria-label="鼠标左侧视图：右侧为前端，侧键 1 在后，侧键 2 在前">
            <defs><clipPath id={clip}><path d="M38 143C40 105 61 66 102 56C148 45 195 70 242 95C271 110 290 114 310 116Q327 118 330 139V151Q328 165 311 165H59Q39 165 38 143Z" /></clipPath></defs>
            <path className="mouse-wheel" d="M270 110L274 103Q277 100 281 104L289 114Z" />
            <path className="mouse-outline" d="M38 143C40 105 61 66 102 56C148 45 195 70 242 95C271 110 290 114 310 116Q327 118 330 139V151Q328 165 311 165H59Q39 165 38 143Z" />
            <g clipPath={`url(#${clip})`}>
              <path className="mouse-seams" d="M42 139Q82 104 135 101C197 97 256 132 330 134M205 78L194 111M39 151H330" />
              <path className="mouse-grip" d="M65 135L83 124M73 141L96 127M84 146L108 132" />
            </g>
            <text className="mouse-orientation" x="45" y="194">后端</text><text className="mouse-orientation" x="317" y="194" textAnchor="end">前端 →</text>
          </svg>
          {[1, 2].map(number => <button key={number} type="button" className={`mouse-side-key mouse-key-${number}`} disabled={!configurable || (number === 2 && mouse.buttons < 5)} aria-label={`配置侧键 ${number}`} aria-pressed={configurable && selected === number} onClick={() => { setSelected(number); (number === 1 ? first : second).current?.focus(); }}>{number}</button>)}
        </div>
        <figcaption>左侧视图 · {configurable ? "点选侧键配置动作" : "启用后可点选侧键"}</figcaption>
      </figure>
      <div className="ocr-mouse-bindings">{([1, 2] as const).map(number => <label className="ocr-mouse-binding" data-active={configurable && selected === number} key={number}>
        <span><b>{number}</b><strong>侧键 {number}</strong><small>{number === 1 ? "通常为后退" : "通常为前进"}</small></span>
        <select ref={number === 1 ? first : second} aria-label={`侧键 ${number} 的动作`} disabled={!configurable || (number === 2 && mouse.buttons < 5)} value={number === 1 ? value.mouse_button_1 : value.mouse_button_2} onFocus={() => setSelected(number)} onChange={e => patch(number === 1 ? { mouse_button_1: e.target.value as MouseAction } : { mouse_button_2: e.target.value as MouseAction })}>
          <option value="default">默认配置 · 保留原用途</option><option value="toolbar">唤出工具栏</option><option value="capture" disabled={!settings.ocr.enabled}>{settings.ocr.enabled ? "直接截图翻译" : "直接截图翻译 · 模块已关闭"}</option>
        </select>
      </label>)}</div>
    </div>
    <p className="ocr-mouse-note"><ShieldCheck /><span>{!supported && value.mouse_shortcuts ? "设备不可用时暂停接管，保留已保存的绑定。" : "仅在本应用运行期间接管已绑定的侧键，默认配置不拦截。"}退出应用后恢复原用途。修改后点击“保存设置”生效。</span></p>
    <div className="toolbar-customization">
      <h3>工具栏设置</h3>
      <div className="toolbar-customization-grid">
        <figure className="toolbar-preview">
          <div className="toolbar-preview-buttons" aria-label="工具栏样式预览">
            {enabledToolbarButtons(settings).map(id => {
              const { icon: Icon, title } = toolbarButtons[id];
              return <button key={id} type="button" tabIndex={-1} disabled={!previewSelection && (id === "translate" || id === "copy")}><Icon />{title}</button>;
            })}
          </div>
          <label className="toolbar-preview-selection"><input type="checkbox" checked={previewSelection} onChange={e => setPreviewSelection(e.target.checked)} />模拟已选中文字</label>
          <figcaption>预览 · 按右侧顺序排列，保存后应用</figcaption>
        </figure>
        <div>
          <Reorder.Group className="toolbar-module-list" axis="y" values={settings.toolbar_order} onReorder={toolbar_order => onChange({ ...settings, toolbar_order })}>
            {settings.toolbar_order.map((module, index) => <ToolbarModuleRow key={module} module={module} enabled={settings[module].enabled} locked={settings[module].enabled && enabledCount === 1} first={index === 0} last={index === settings.toolbar_order.length - 1} onToggle={() => toggleModule(module)} onMove={direction => moveModule(index, direction)} />)}
          </Reorder.Group>
          <p className="toolbar-module-note">拖动手柄调整顺序，也可使用上下箭头。开关控制对应功能，至少保留一个模块。</p>
        </div>
      </div>
    </div>
  </div>;
}

function ToolbarModuleRow({ module, enabled, locked, first, last, onToggle, onMove }: {
  module: ToolbarModule; enabled: boolean; locked: boolean; first: boolean; last: boolean;
  onToggle: () => void; onMove: (direction: number) => void;
}) {
  const controls = useDragControls();
  const { title, description } = toolbarModules[module];
  return <Reorder.Item className="toolbar-module-row" value={module} dragListener={false} dragControls={controls}>
    <button type="button" className="toolbar-drag-handle" aria-label={`拖动${title}排序`} onPointerDown={e => controls.start(e)} onKeyDown={e => { if (e.key === "ArrowUp" && !first) { e.preventDefault(); onMove(-1); } if (e.key === "ArrowDown" && !last) { e.preventDefault(); onMove(1); } }}><GripVertical /></button>
    <div className="toolbar-module-copy"><strong>{title}</strong><p>{description}</p></div>
    <div className="toolbar-order-buttons"><button type="button" aria-label={`上移${title}`} disabled={first} onClick={() => onMove(-1)}><ArrowUp /></button><button type="button" aria-label={`下移${title}`} disabled={last} onClick={() => onMove(1)}><ArrowDown /></button></div>
    <label className="ocr-entry-switch" title={locked ? "至少保留一个模块" : undefined}><input type="checkbox" aria-label={`启用${title}`} checked={enabled} disabled={locked} onChange={onToggle} /><span /></label>
  </Reorder.Item>;
}
