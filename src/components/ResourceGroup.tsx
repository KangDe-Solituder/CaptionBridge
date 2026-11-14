import { ChevronRight } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useId, useState, type ReactNode } from "react";
import { HelpTip } from "./HelpTip";

export function ResourceGroup({ title, hint, actions, children, reduceMotion }: {
  title: string; hint?: string; actions?: ReactNode; children: ReactNode; reduceMotion: boolean;
}) {
  const [open, setOpen] = useState(true);
  const id = useId();
  const systemReduced = useReducedMotion();
  return <section className="resource-group">
    <div className="resource-group-header">
      <div className="resource-group-title"><button type="button" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <ChevronRight size={17} style={{ transform: open ? "rotate(90deg)" : undefined }} />{title}
      </button><HelpTip text={hint} /></div>
      {actions && <div className="resource-actions">{actions}</div>}
    </div>
    <motion.div id={id} initial={false} animate={{ height: open ? "auto" : 0, opacity: open ? 1 : 0 }}
      transition={{ duration: reduceMotion || systemReduced ? 0 : .23, ease: [.22, 1, .36, 1] }}
      inert={!open} aria-hidden={!open} className="resource-group-reveal">
      <div className="resource-group-content">{children}</div>
    </motion.div>
  </section>;
}
