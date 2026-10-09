import { useMemo } from "react";
import { t } from "../i18n";
import { wordDiff, type DiffSegment } from "./textDiff";

/** `after` with the words added since `before` highlighted and the removed ones struck through. */
export function DiffText({ before, after }: { before: string; after: string }) {
  const segments = useMemo(() => wordDiff(before, after), [before, after]);
  return <DiffSegments segments={segments}/>;
}

function DiffSegments({ segments }: { segments: DiffSegment[] }) {
  return (
    <>
      {segments.map((seg, idx) => {
        if (seg.change === "keep") return <span key={idx}>{seg.text}</span>;
        if (seg.change === "add") return <span key={idx} style={{ background: "color-mix(in srgb, var(--ok) 18%, transparent)", color: "var(--ok)", borderRadius: 2 }}>{seg.text}</span>;
        return <span key={idx} style={{ background: "color-mix(in srgb, var(--err) 18%, transparent)", color: "var(--err)", textDecoration: "line-through", borderRadius: 2 }}>{seg.text}</span>;
      })}
    </>
  );
}

export function DiffBlock({ before, after, title = t("Diff: до LLM → финальный") }: { before: string; after: string; title?: string }) {
  // With no changes every segment is "keep", so the block renders as plain
  // unhighlighted text — indistinguishable from broken highlighting. Say so.
  const segments = useMemo(() => wordDiff(before, after), [before, after]);
  const unchanged = segments.every((seg) => seg.change === "keep");
  return (
    <div style={{ marginTop: 10, padding: 10, borderRadius: "var(--radius-sm)", background: "var(--bg-3)", border: "1px solid var(--line)" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8, marginBottom: 6 }}>
        <div style={{ font: "600 10px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
          {title}
        </div>
        {unchanged ? (
          <div style={{ font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)", textTransform: "uppercase", letterSpacing: "0.04em", whiteSpace: "nowrap" }}>
             {t("без изменений")} </div>
        ) : null}
      </div>
      <div style={{ font: "400 13px/1.5 var(--font-sans)", color: unchanged ? "var(--ink-mute)" : "var(--ink)", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}>
        <DiffSegments segments={segments}/>
      </div>
    </div>
  );
}
