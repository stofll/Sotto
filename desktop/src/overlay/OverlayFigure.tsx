import { lazy, Suspense, useEffect, useRef } from "react";
import { t } from "../i18n";
import type { ClockDraw } from "./clockPainters";
import type { RegionKind } from "./overlayRecipe";
import { countWords, limitProgress, wordsPerMinute, type SceneFigures } from "./recordingFigures";

const OverlayClock = lazy(() => import("./OverlayClock").then((m) => ({ default: m.OverlayClock }))
  .catch(() => ({ default: () => <></> })));

export type FigureType = "limit" | "words" | "pace" | "engine" | "label" | "target";

type FigureProps = {
  type: FigureType;
  kind: RegionKind;
  /** The side of a square region, px. */
  box: number;
  figures: SceneFigures;
  draft: string;
  /** A streaming model is running, so the draft means something. */
  streaming: boolean;
  limited: boolean;
  label: string;
  /** Shown instead of an empty label (the constructor), so the region is not a blank. */
  labelPlaceholder?: string;
};

const RING_RADIUS = 11;
const RING_LENGTH = 2 * Math.PI * RING_RADIUS;

/** The record dot inside a ring that closes towards the recording limit. Without a limit the ring stays open. */
function LimitRing({ progress, limited, size }: { progress: number | null; limited: boolean; size: number }) {
  return <svg className="ovs-limit" width={size} height={size} viewBox="0 0 28 28" data-warn={limited ? "1" : undefined} aria-hidden="true">
    <circle className="ovs-limit__track" cx="14" cy="14" r={RING_RADIUS}/>
    {progress !== null && <circle className="ovs-limit__arc" cx="14" cy="14" r={RING_RADIUS} strokeDasharray={RING_LENGTH.toFixed(2)}
      strokeDashoffset={(RING_LENGTH * (1 - progress)).toFixed(2)} transform="rotate(-90 14 14)"/>}
    <circle className="ovs-limit__dot" cx="14" cy="14" r="4"/>
  </svg>;
}

/** The data elements of a recipe: limit ring, word count, pace, engine, the user's label and the target application. */
export function FigurePart({ type, kind, box, figures, draft, streaming, limited, label, labelPlaceholder }: FigureProps) {
  switch (type) {
    case "limit": {
      const size = kind === "square" ? Math.round(box * 0.62) : 28;
      return <LimitRing progress={limitProgress(figures.elapsed, figures.limitSeconds)} limited={limited} size={size}/>;
    }
    case "words":
      return <span className="ovs-fig"><b>{streaming ? countWords(draft) : "—"}</b><span>{t("слов")}</span></span>;
    case "pace": {
      const rate = streaming ? wordsPerMinute(countWords(draft), figures.elapsed) : null;
      return <span className="ovs-fig"><b>{rate ?? "—"}</b><span>{t("сл/мин")}</span></span>;
    }
    case "engine":
      return <span className="ovs-fig"><i className="ovs-fig__dot" data-tone={figures.cloud ? "warn" : "ok"}/><b>{figures.cloud ? t("облако") : t("локально")}</b></span>;
    case "label":
      return label || !labelPlaceholder
        ? <span className="ovs-fig"><b>{label}</b></span>
        : <span className="ovs-fig ovs-fig--empty">{labelPlaceholder}</span>;
    case "target":
      return <span className="ovs-fig"><span aria-hidden="true">→</span><b>{figures.target || "—"}</b></span>;
  }
}

/** Flip cards: a digit that changes turns over once; the others keep still. */
function FlipClock({ text }: { text: string }) {
  const previous = useRef(text);
  const chars = Array.from(text);
  const before = previous.current;
  useEffect(() => { previous.current = text; }, [text]);
  return <span className="ovs-flip" aria-hidden="true">
    {chars.map((char, index) => <span key={`${index}${char}`} className={char === ":" ? "ovs-flip__colon" : char !== Array.from(before)[index] ? "ovs-flip__go" : undefined}>{char}</span>)}
  </span>;
}

/** The timer as a flip clock or one of the canvas clocks. The text stays for a screen reader. */
export function ClockPart({ draw, text, warn }: { draw: ClockDraw | "flip"; text: string; warn: boolean }) {
  return <>
    <span className="sr-only">{text}</span>
    {draw === "flip" ? <FlipClock text={text}/> : <Suspense fallback={null}><OverlayClock draw={draw} text={text} warn={warn}/></Suspense>}
  </>;
}
