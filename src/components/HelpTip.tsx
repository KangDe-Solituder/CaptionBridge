import { CircleHelp } from "lucide-react";
import { useId, useState } from "react";

export function HelpTip({ text }: { text?: string }) {
  const id = useId();
  const [position, setPosition] = useState<{ left: number; top: number }>();
  if (!text) return null;
  const show = (element: HTMLElement) => { const r = element.getBoundingClientRect(); setPosition({ left: Math.max(8, Math.min(r.left, window.innerWidth - 328)), top: Math.max(8, Math.min(r.bottom + 8, window.innerHeight - 150)) }); };
  return <span className="help-tip" tabIndex={0} role="img" aria-label={text} aria-describedby={position ? id : undefined}
    onMouseEnter={e => show(e.currentTarget)} onMouseLeave={() => setPosition(undefined)}
    onFocus={e => show(e.currentTarget)} onBlur={() => setPosition(undefined)}
    onKeyDown={e => { if (e.key === "Escape") setPosition(undefined); }}
    onClick={e => { e.preventDefault(); e.stopPropagation(); }}>
    <CircleHelp size={15} />{position && <span id={id} role="tooltip" className="help-tip-content" style={position}>{text}</span>}
  </span>;
}
