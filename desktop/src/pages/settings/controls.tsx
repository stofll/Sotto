// Pieces every settings section needs: the label/hint primitives and the type
// of the saver the sections are handed.

import { Hint } from "../../components/Hint";
import type { ConfigResult } from "../../bridge/types";

/** What a settings control calls to persist one change: the page's own saver,
 *  which answers with the config as Rust wrote it. Every section takes it, so
 *  it is declared once here rather than per file. */
export type ConfigChanged = (patch: Partial<ConfigResult>) => Promise<ConfigResult | null>;

export function HintIcon({ text }: { text: string }) {
  return <Hint text={text}/>;
}

export function SetLabel({ title, hint }: { title: string; hint?: string }) {
  return (
    <span className="set-label">
      {title}
      {hint && <HintIcon text={hint}/>}
    </span>
  );
}
