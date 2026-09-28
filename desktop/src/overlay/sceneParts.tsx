import { lazy, Suspense, useEffect, useId, useRef, type CSSProperties, type RefObject } from "react";
import { t } from "../i18n";
import { OverlayWaveform } from "./OverlayWaveform";
import { easeStep, frameRunner, type FrameTick } from "./frameClock";
import type { LevelSource } from "./levelSource";
import type { OverlaySize, ProcessDraw, RecipeMatrix, RegionKind, ScenePhase } from "./overlayRecipe";

// The heavier drawings load on first use, so a recipe without them costs nothing.
const OverlayMatrix = lazy(() => import("./OverlayMatrix").then((m) => ({ default: m.OverlayMatrix }))
  .catch(() => ({ default: () => <></> })));
const OverlayGlow = lazy(() => import("./OverlayGlow").then((m) => ({ default: m.OverlayGlow }))
  .catch(() => ({ default: () => <></> })));

const BLOCKS = "▁▂▃▄▅▆▇█";
const HISTORY = 96;

/** The last `HISTORY` readings, pushed to `paint` as they arrive. */
function useReadings(source: LevelSource, paint: (levels: number[]) => void) {
  const paintRef = useRef(paint);
  paintRef.current = paint;
  useEffect(() => {
    const levels: number[] = Array(HISTORY).fill(0);
    paintRef.current(levels);
    return source((level) => {
      levels.push(level);
      levels.shift();
      paintRef.current(levels);
    });
  }, [source]);
}

/** The same history, but painted on the shared ~60 fps clock while readings
 *  keep coming, for drawings that move smoothly between two readings. */
function useLevelFrames(source: LevelSource, tick: (levels: number[], now: number, seconds: number) => void) {
  const tickRef = useRef(tick);
  tickRef.current = tick;
  useEffect(() => {
    const levels: number[] = Array(HISTORY).fill(0);
    const runner = frameRunner(((now, seconds) => tickRef.current(levels, now, seconds)) as FrameTick);
    runner.wake();
    const stop = source((level) => {
      levels.push(level);
      levels.shift();
      runner.wake();
    });
    return () => { stop(); runner.stop(); };
  }, [source]);
}

type LevelProps = {
  draw: string;
  kind: RegionKind;
  /** The side of the square or tall region, px. */
  box: number;
  source: LevelSource;
  matrix: RecipeMatrix;
  sharp: boolean;
  size: OverlaySize;
  phase: ScenePhase;
  surfaceRef?: RefObject<HTMLDivElement | null>;
};

export function LevelPart({ draw, kind, box, source, matrix, sharp, size, phase, surfaceRef }: LevelProps) {
  const small = kind === "small";
  switch (draw) {
    case "bars": case "wave": case "qbars":
      return <div className={`ovs-level ovs-level--${small ? "small" : "fill"}`}>
        <OverlayWaveform source={source} surfaceRef={surfaceRef} variant={draw === "qbars" ? "pixel" : draw === "wave" ? "wave" : "bars"}/>
      </div>;
    case "ring": {
      // Only the bead's core is sized for the full ring. Elsewhere it is compact: a bar
      // reaches 3 px past the radius, so a ring of radius 11 fits the small orb's 30 px.
      const compact = kind !== "square";
      return <div className={`ovs-ring${compact ? " ovs-ring--small" : ""}`} style={{ "--overlay-ring-radius": `${compact ? 11 : Math.round(box * 0.36)}px` } as CSSProperties}>
        <OverlayWaveform source={source} surfaceRef={surfaceRef} circular compact={compact}/>
      </div>;
    }
    case "matrix": {
      const grid = <Suspense fallback={null}>
        <OverlayMatrix mode="speech" source={source} surfaceRef={surfaceRef} sharp={sharp} {...matrix}
          size={kind === "wide" || small ? undefined : Math.round(box * (kind === "square" ? 0.66 : 0.7))}/>
      </Suspense>;
      // A row fills its box, and a small region sizes to its content, so it needs a box of its own.
      return small ? <div className="ovs-level ovs-level--small">{grid}</div> : grid;
    }
    case "beam":
      return <Suspense fallback={null}>
        <OverlayGlow mode={phase === "recording" ? "listen" : phase === "processing" ? "process" : "idle"} size={size} source={source}/>
      </Suspense>;
    case "scope": return small ? <div className="ovs-level ovs-level--small"><Scope source={source}/></div> : <Scope source={source}/>;
    case "ascii": return <Ascii source={source} count={small ? 7 : 26}/>;
    case "caps": return <Capsules source={source} kind={kind} box={box}/>;
    case "segments": return <Segments source={source} unit={small || kind === "wide" ? 7 : Math.max(5, Math.round(box * 0.14))}/>;
    case "orb": return <Orb source={source} side={small || kind === "wide" ? 30 : Math.round(box * (kind === "square" ? 1 : 0.7))}/>;
    default: return null;
  }
}

function Scope({ source }: { source: LevelSource }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const energy = useRef(0.08);
  useLevelFrames(source, (levels, now, seconds) => {
    energy.current += (Math.sqrt(levels[levels.length - 1]) - energy.current) * easeStep(seconds, 0.12);
    const canvas = ref.current, context = canvas?.getContext("2d");
    if (!canvas || !context || !canvas.clientWidth) return;
    const ratio = window.devicePixelRatio || 1, width = Math.round(canvas.clientWidth * ratio), height = Math.round(canvas.clientHeight * ratio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    // Fade the last frame instead of clearing it: the trace keeps a short afterglow.
    context.globalCompositeOperation = "destination-out";
    context.fillStyle = "rgba(0, 0, 0, 0.34)";
    context.fillRect(0, 0, width, height);
    context.globalCompositeOperation = "source-over";
    const colour = getComputedStyle(canvas).getPropertyValue("--overlay-wave-mid").trim() || "#ff8a3d";
    const amplitude = height * 0.42 * Math.min(1, energy.current * 1.25), time = now / 1000;
    context.lineWidth = 1.5 * ratio; context.strokeStyle = colour; context.shadowColor = colour; context.shadowBlur = 5 * ratio;
    context.beginPath();
    for (let x = 0; x <= width; x += 3) {
      const u = x / width, envelope = Math.pow(Math.sin(Math.PI * u), 0.6);
      const y = height / 2 + amplitude * envelope * (Math.sin(u * 18 + time * 6) * 0.55 + Math.sin(u * 41 - time * 9) * 0.3 + Math.sin(u * 83 + time * 15) * 0.15);
      if (x) context.lineTo(x, y); else context.moveTo(x, y);
    }
    context.stroke();
    context.shadowBlur = 0;
  });
  return <canvas ref={ref} className="ovs-scope" aria-hidden="true"/>;
}

function Ascii({ source, count }: { source: LevelSource; count: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  useReadings(source, (levels) => {
    if (ref.current) ref.current.textContent = levels.slice(-count).map((level) => BLOCKS[Math.min(7, Math.floor(Math.sqrt(level) * 8))]).join("");
  });
  return <span ref={ref} className="ovs-ascii" aria-hidden="true"/>;
}

function Capsules({ source, kind, box }: { source: LevelSource; kind: RegionKind; box: number }) {
  const refs = useRef<(HTMLElement | null)[]>([]);
  const unit = kind === "small" ? 6 : kind === "wide" ? 8 : Math.max(5, Math.round(box * 0.13));
  const tallest = kind === "small" ? 22 : kind === "wide" ? 26 : box * 0.62;
  // Each capsule answers a slightly older reading at its own gain, so they move as a group, not in lockstep.
  const lag = [3, 0, 1, 5], gain = [0.75, 1, 0.9, 0.7];
  const heights = useRef([0, 0, 0, 0]);
  useLevelFrames(source, (levels, _now, seconds) => refs.current.forEach((capsule, index) => {
    const target = Math.sqrt(levels[levels.length - 1 - lag[index]]) * gain[index];
    heights.current[index] += (target - heights.current[index]) * easeStep(seconds, 0.045);
    if (capsule) capsule.style.height = `${(unit + heights.current[index] * (tallest - unit)).toFixed(1)}px`;
  }));
  return <div className="ovs-caps" style={{ "--u": `${unit}px` } as CSSProperties} aria-hidden="true">
    {lag.map((_, index) => <i key={index} ref={(node) => { refs.current[index] = node; }}/>)}
  </div>;
}

function Segments({ source, unit }: { source: LevelSource; unit: number }) {
  const refs = useRef<(HTMLElement | null)[]>([]);
  useReadings(source, (levels) => {
    const level = levels[levels.length - 1], value = Math.sqrt(level);
    const lit = level < 0.03 ? 0 : value < 0.45 ? 1 : value < 0.72 ? 2 : 3;
    refs.current.forEach((segment, index) => { if (segment) segment.dataset.on = String(index < lit); });
  });
  return <div className="ovs-seg" style={{ "--u": `${unit}px` } as CSSProperties} aria-hidden="true">
    {[0, 1, 2].map((index) => <i key={index} ref={(node) => { refs.current[index] = node; }}/>)}
  </div>;
}

/**
 * A drop of liquid that answers the voice. Everything follows a smoothed
 * level, so the drop swells and settles instead of twitching with each
 * reading. The blobs circle on their own slow ellipses, half of them the
 * other way round, and those paths drift over seconds, so the figure keeps
 * changing without jumping. Silence draws the blobs into the core, which
 * then only breathes. The goo filter's blur follows the size, otherwise a
 * small orb blurs below the alpha threshold and disappears when the room is
 * quiet.
 */
function Orb({ source, side }: { source: LevelSource; side: number }) {
  const filter = `ovs-goo-${useId().replace(/:/g, "")}`;
  const refs = useRef<(HTMLElement | null)[]>([]);
  const motion = useRef({ smooth: 0, angle: 0, own: [0, 0, 0, 0] });
  const core = Math.round(side * 0.52), blob = Math.round(side * 0.28);
  useLevelFrames(source, (readings, now, seconds) => {
    const state = motion.current, level = Math.sqrt(readings[readings.length - 1] ?? 0);
    state.smooth += (level - state.smooth) * easeStep(seconds, 0.18);
    state.angle += seconds * (0.35 + state.smooth * 0.9);
    const [ball, ...blobs] = refs.current;
    const breath = 1 + 0.035 * Math.sin(now / 1100);
    if (ball) ball.style.transform = `scale(${(breath + state.smooth * 0.12).toFixed(3)})`;
    blobs.forEach((node, index) => {
      const i = index + 1, direction = i % 2 ? 1 : -1;
      // Each blob answers a slightly older reading, so they swell one after another.
      state.own[index] += (Math.sqrt(readings[readings.length - 1 - i * 3] ?? 0) - state.own[index]) * easeStep(seconds, 0.22);
      const own = state.own[index];
      const drift = Math.sin(now / (5200 + i * 1300) + i * 1.7);
      const a = direction * state.angle * (0.8 + i * 0.15) + i * 1.57 + drift * 0.6;
      const reach = Math.min(0.5, 0.04 + state.smooth * (0.26 + own * 0.16)) * side;
      // An ellipse that slowly turns instead of a fixed circle.
      const wide = 1 + 0.22 * drift, tilt = i * 0.8 + now / 9000;
      const x = Math.cos(a) * reach * wide, y = Math.sin(a) * reach / wide;
      const grow = 0.55 + state.smooth * 0.4 + own * 0.2;
      if (node) node.style.transform = `translate(${(x * Math.cos(tilt) - y * Math.sin(tilt)).toFixed(2)}px,${(x * Math.sin(tilt) + y * Math.cos(tilt)).toFixed(2)}px) scale(${grow.toFixed(3)})`;
    });
  });
  return <div className="ovs-orb" style={{ width: side, height: side, filter: `url(#${filter})` }} aria-hidden="true">
    <svg width="0" height="0" style={{ position: "absolute" }}>
      <filter id={filter} x="-50%" y="-50%" width="200%" height="200%">
        <feGaussianBlur in="SourceGraphic" stdDeviation={Math.max(1.2, side * 0.06).toFixed(2)}/>
        <feColorMatrix values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 19 -8"/>
      </filter>
    </svg>
    {[0, 1, 2, 3, 4].map((index) => <i key={index} ref={(node) => { refs.current[index] = node; }}
      style={{ "--d": `${index ? blob : core}px` } as CSSProperties}/>)}
  </div>;
}

export function TimerPart({ draw, kind, text }: { draw: string; kind: RegionKind; text: string }) {
  if (draw === "capsule") return <div className="ovs-tcap"><span className="ovs-dot"/><span className="ovs-tt">{text}</span></div>;
  return <div className={draw === "big" ? "ovs-tbig" : "ovs-tplain"}>
    {kind === "tall" && draw === "plain" && <span className="ovs-dot"/>}<span className="ovs-tt">{text}</span>
  </div>;
}

export function RecPart({ draw }: { draw: string }) {
  if (draw === "label") return <div className="ovs-rlabel"><span className="ovs-dot"/><span>{t("Слушаю")}</span></div>;
  if (draw === "REC") return <span className="ovs-rec-text">● REC</span>;
  return <div className="ovs-rdot"><span className="ovs-dot"/></div>;
}

export function ModePart({ draw, full, short }: { draw: string; full: string; short: string }) {
  return <span className="ovs-mode">{draw === "chip" ? full : short}</span>;
}

/** The streaming draft. `tail` draws the last two words softer: they are the
 *  part of the hypothesis most likely to change. */
export function DraftPart({ draw, text, captions, phase, placeholder }: { draw: string; text: string; captions: boolean; phase: ScenePhase; placeholder: string }) {
  const words = text ? text.split(/\s+/).filter(Boolean) : [];
  const cut = draw === "tail" && phase === "recording" ? Math.max(0, words.length - 2) : words.length;
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { if (ref.current) ref.current.scrollTop = ref.current.scrollHeight; }, [text]);
  if (captions) {
    return <div className="ovs-flow" ref={ref}>
      {words.length > 0 && <span className="ovs-inl">
        {words.map((word, index) => <span key={index} className={`ovs-w${index >= cut ? " ovs-w--tail" : ""}`} style={{ "--i": index } as CSSProperties}>{word}{index < words.length - 1 ? " " : ""}</span>)}
      </span>}
    </div>;
  }
  if (!words.length) return <div className="ovs-draft ovs-draft--empty" ref={ref}>{placeholder}</div>;
  return <div className="ovs-draft" ref={ref}>
    {words.slice(0, cut).join(" ")}{cut < words.length && <> <span className="ovs-draft__tail">{words.slice(cut).join(" ")}</span></>}
  </div>;
}

/** The processing look the recipe chose, `size` px tall: a row's height, or the middle of a round shell. */
export function ProcessLead({ draw, matrix, size, sharp, pace = 1 }: { draw: Exclude<ProcessDraw, "none">; matrix: RecipeMatrix; size: number; sharp: boolean; pace?: number }) {
  switch (draw) {
    case "matrix": return <Suspense fallback={null}><OverlayMatrix mode="process" size={size} sharp={sharp} pace={pace} {...matrix}/></Suspense>;
    case "arc": return <span className="ovs-comet" style={{ width: size, height: size }}/>;
    case "cursor": return <span className="ovs-blk" style={{ "--blk-w": `${Math.max(6, Math.round(size * 0.36))}px`, "--blk-h": `${Math.max(12, Math.round(size * 0.64))}px` } as CSSProperties}/>;
    case "dots": return <span className="ovs-dots" style={{ "--dot": `${Math.max(5, Math.round(size * 0.2))}px` } as CSSProperties}><i/><i/><i/></span>;
  }
}

export function CheckMark({ big = false }: { big?: boolean }) {
  return <svg className={`ovs-chk${big ? " ovs-chk--big" : ""}`} viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.2 4.2L19 7"/></svg>;
}
