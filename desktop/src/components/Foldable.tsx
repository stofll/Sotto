import type { ReactNode } from "react";
import { Hint } from "./Hint";
import { Icon } from "./Icon";

/** A collapsible card on the «Текст» page.
 *
 * The header is split into two zones: the collapse button (icon, name, summary)
 * and an `aside` beside it. The switches live in `aside` on purpose — inside the
 * button a click on a switch would collapse the card as well. The `hint` icon
 * sits beside the button for the same reason: asking what the card does should
 * not fold it away. */
export function Foldable({ open, title, summary, hint, aside, onToggle, children }: { open: boolean; title: string; summary?: ReactNode; hint?: string; aside?: ReactNode; onToggle: () => void; children: ReactNode }) {
  return (
    <section className="card fold">
      <div className="fold__head">
        <div className="fold__heading">
          <button type="button" className="fold__toggle" onClick={onToggle} aria-expanded={open}>
            <span className="fold__chev" data-open={open ? "true" : "false"}><Icon name="chev-right" size={13}/></span>
            <span className="fold__title">{title}</span>
            {summary}
          </button>
          {hint && <Hint text={hint}/>}
        </div>
        {aside && <div className="fold__aside">{aside}</div>}
      </div>
      {open && <div className="fold__body">{children}</div>}
    </section>
  );
}

