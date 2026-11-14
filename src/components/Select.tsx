import { Check, ChevronDown } from "lucide-react";
import { forwardRef, useEffect, useId, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

type Option = { value: string; label: string; disabled?: boolean };
type Props = {
  value: string | number; options: Option[]; onChange: (value: string) => void;
  "aria-label": string; disabled?: boolean; autoFocus?: boolean; onFocus?: () => void;
};

/** Select-only combobox. The popup stays in its modal's top layer. */
export const Select = forwardRef<HTMLButtonElement, Props>(function Select(props, ref) {
  const { value, options, onChange, disabled, autoFocus, onFocus } = props;
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const id = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [position, setPosition] = useState({ left: 0, top: 0, width: 0, maxHeight: 280 });
  const search = useRef({ text: "", at: 0 });
  const selected = options.findIndex(option => option.value === String(value));
  useImperativeHandle(ref, () => trigger.current!, []);

  const show = (last = false) => {
    if (disabled) return;
    const first = options.findIndex(option => !option.disabled);
    const lastEnabled = options.map((option, index) => option.disabled ? -1 : index).filter(index => index >= 0).at(-1) ?? -1;
    setActive(selected >= 0 && !options[selected].disabled ? selected : last ? lastEnabled : first);
    setOpen(true);
  };
  const choose = (index: number) => {
    const option = options[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    setOpen(false);
    trigger.current?.focus();
  };
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  useLayoutEffect(() => {
    if (!open || !trigger.current) return;
    const place = () => {
      const r = trigger.current!.getBoundingClientRect();
      const below = window.innerHeight-r.bottom-14;
      const above = r.top-14;
      const height = Math.min(280, options.length*38+12);
      const up = below < height && above > below;
      const maxHeight = Math.max(40, Math.min(height, up ? above : below));
      const width = Math.min(Math.max(r.width, 170), window.innerWidth-20);
      setPosition({ left: Math.max(10, Math.min(r.left, window.innerWidth-width-10)), top: up ? r.top-maxHeight-6 : r.bottom+6, width, maxHeight });
    };
    place();
    const outside = (e: PointerEvent) => {
      if (!trigger.current?.contains(e.target as Node) && !popup.current?.contains(e.target as Node)) setOpen(false);
    };
    const scroll = (e: Event) => { if (!popup.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    return () => { document.removeEventListener("pointerdown", outside, true); window.removeEventListener("resize", place); window.removeEventListener("scroll", scroll, true); };
  }, [open, options.length]);
  useEffect(() => {
    if (open) popup.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  return <>
    <button ref={trigger} type="button" className="select-trigger" role="combobox" aria-label={props["aria-label"]}
      aria-expanded={open} aria-controls={open ? id : undefined} aria-haspopup="listbox"
      aria-activedescendant={open && active >= 0 ? `${id}-${active}` : undefined}
      disabled={disabled} autoFocus={autoFocus} onFocus={onFocus}
      onBlur={() => setOpen(false)} onClick={() => open ? setOpen(false) : show()}
      onKeyDown={e => {
        if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); setOpen(false); return; }
        if (e.key === "Tab") { setOpen(false); return; }
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
          e.preventDefault();
          if (!open) { show(e.key === "ArrowUp" || e.key === "End"); return; }
          const enabled = options.map((o, i) => o.disabled ? -1 : i).filter(i => i >= 0);
          const at = enabled.indexOf(active);
          setActive(e.key === "Home" ? enabled[0] : e.key === "End" ? enabled.at(-1)! : enabled[(at+(e.key === "ArrowDown" ? 1 : -1)+enabled.length)%enabled.length] ?? -1);
        } else if (e.key === "Enter" || e.key === " ") {
          e.preventDefault(); if (open) choose(active); else show();
        } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          const now = Date.now();
          search.current = { text: (now-search.current.at < 700 ? search.current.text : "")+e.key.toLowerCase(), at: now };
          const index = options.findIndex(o => !o.disabled && o.label.toLowerCase().startsWith(search.current.text));
          if (index >= 0) { if (!open) show(); setActive(index); }
        }
      }}><span>{options[selected]?.label ?? "请选择"}</span><ChevronDown size={15} /></button>
    {open && createPortal(<div ref={popup} id={id} role="listbox" aria-label={props["aria-label"]} className="select-menu" style={position}
      onPointerDown={e => e.preventDefault()}>
      {options.map((option, index) => <div key={option.value} id={`${id}-${index}`} role="option" aria-selected={index === selected}
        aria-disabled={option.disabled || undefined} data-index={index} data-active={index === active}
        onPointerMove={() => { if (!option.disabled) setActive(index); }} onClick={() => choose(index)}>
        <span>{option.label}</span>{index === selected && <Check size={15} />}
      </div>)}
    </div>, trigger.current?.closest("dialog") ?? document.body)}
  </>;
});
