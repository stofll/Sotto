import { useMemo } from "react";
import { t } from "../i18n";
import { wordDiff } from "./textDiff";

export function DiffBlock({ before, after, title = t("Diff: до LLM → финальный") }: { before: string; after: string; title?: string }) {
  const segments = useMemo(() => wordDiff(before, after), [before, after]);
  // With no changes every segment is "keep", so the block renders as plain
  // unhighlighted text — indistinguishable from broken highlighting. Say so.
  const unchanged = useMemo(() => segments.every((seg) => seg.change === "keep"), [segments]);
  return (
    <div style={{ marginTop: 10, padding: 10, borderRadius: "var(--r-sm)", background: "var(--surface-1)", border: "1px solid var(--border)" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8, marginBottom: 6 }}>
        <div style={{ font: "600 10px/1 var(--font-mono)", color: "var(--text-mute)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
          {title}
        </div>
        {unchanged ? (
          <div style={{ font: "500 10px/1 var(--font-mono)", color: "var(--text-mute)", textTransform: "uppercase", letterSpacing: "0.04em", whiteSpace: "nowrap" }}>
             {t("без изменений")} </div>
        ) : null}
      </div>
      <div style={{ font: "400 13px/1.5 var(--font-sans)", color: unchanged ? "var(--text-mute)" : "var(--text)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}>
        {segments.map((seg, idx) => {
          if (seg.change === "keep") return <span key={idx}>{seg.text}</span>;
          if (seg.change === "add") return <span key={idx} style={{ background: "rgba(56,205,127,0.18)", color: "var(--ok)", borderRadius: 2 }}>{seg.text}</span>;
          return <span key={idx} style={{ background: "rgba(239,94,107,0.18)", color: "var(--err)", textDecoration: "line-through", borderRadius: 2 }}>{seg.text}</span>;
        })}
      </div>
    </div>
  );
}
