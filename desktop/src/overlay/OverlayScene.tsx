import { useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { Icon } from "../components/Icon";
import { dissolveAllowed, useShellDissolve } from "./shellDissolve";
import { t } from "../i18n";
import { CheckMark, DraftPart, FigurePart, FillLayers, LevelPart, ModePart, ProcessLead, RecPart, RingRim, TimerPart } from "./sceneParts";
import { ringClip, ringHoleSize } from "./shellShapes";
import { sampleFigures, type SceneFigures } from "./recordingFigures";
import type { SmokeFrame } from "./fillPainters";
import type { LevelSource } from "./levelSource";
import {
  COMPACT_SHELLS, FONT_SCALE, NOTCH_EAR, PROCESS_PACE, SHELL_LAYOUT, SHELL_REGIONS, WINDOW_SIZE, firstKind, recipeLayout, regionOf, shellRadius, shellSize,
  type ElementType, type NotchFit, type OverlaySize, type Recipe, type ScenePhase, type Shell,
} from "./overlayRecipe";
import "./overlay.css";
import "./overlayScene.css";

export type SceneProps = {
  recipe: Recipe;
  size: OverlaySize;
  phase: ScenePhase;
  /** False while the overlay is hidden between sessions (the constructor's scenario). */
  shown?: boolean;
  /** A streaming model is running, so the draft row may open. */
  streaming: boolean;
  /** An error or a warning that needs the pill row's width. */
  needsText?: boolean;
  draft: string;
  draftPlaceholder: string;
  timer: string;
  limited: boolean;
  /** The camera notch the notch shell sits around; none draws the band's own centre (no notch, or the constructor's stage). */
  notch?: NotchFit | null;
  /** What the data elements (limit, words, pace, engine, target) show; thumbnails get a sample. */
  figures?: SceneFigures;
  /** The words of the current state: "processing", a pasted count, an error. */
  status: ReactNode;
  mode: { full: string; short: string };
  source: LevelSource;
  /** `label` names the action for screen readers; `text` is the word the "text" drawing shows. */
  close: { label: string; text: string; onClick?: () => void; disabled?: boolean };
  hovered?: boolean;
  surfaceRef?: RefObject<HTMLDivElement | null>;
  /** Constructor mode: regions become targets with labels, and the targets outside the shell appear. */
  interactive?: { labels: Record<string, string>; ghosts: Record<string, string>; selected: ElementType | "cancel" | null; selectedRegion?: string | null; cancelLabel: string; hideCancel?: boolean };
  /** The overlay is on its way out (the native "overlay-leaving"); with the dissolve exit the pasted note goes through a dither mask. */
  leaving?: boolean;
  /** Thumbnails and paused previews: no ongoing animation. */
  still?: boolean;
};

/** How much of the shell's side a status sign takes when the shell is compact. */
const SIGN_SCALE: Partial<Record<Shell, number>> = { mini: 0.8, ring: 0.42, notch: 0.7 };
/** How many characters of the decoding text fit a shell's status row. */
const DECODE_CHARS: Partial<Record<Shell, number>> = { island: 14, card: 30 };

export function OverlayScene(props: SceneProps) {
  const { recipe, size, phase, streaming, source, interactive, still } = props;
  const shown = props.shown ?? true;
  const needsText = props.needsText ?? phase === "error";
  const draftOpen = phase === "recording" && streaming && recipe.slots.below === "draft";
  const layout = recipeLayout(recipe, draftOpen, needsText);
  const [shellWidth, shellHeight] = shellSize(recipe, size, layout, phase, shown, props.notch);
  const pad = recipe.style.stroke === "rim" ? 4 : 0;
  const [designWidth, designHeight] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][size];
  const box = recipe.shell === "stack" ? designWidth - 8 - 2 * pad : Math.min(designWidth, designHeight) - 8 - 2 * pad;
  const rowHeight = WINDOW_SIZE.pill[size][1] - 8 - 2 * pad;
  const compact = COMPACT_SHELLS.includes(recipe.shell);
  const regions = SHELL_REGIONS[recipe.shell];
  const localSurface = useRef<HTMLDivElement>(null);
  const surfaceRef = props.surfaceRef ?? localSurface;
  const rootRef = useRef<HTMLDivElement>(null);
  // The split's capsules show two parts of one smoke field; the first one moves it on.
  const splitRef = useRef<HTMLDivElement>(null);
  const splitSmoke = useRef<SmokeFrame>({ time: 0, drift: 0, energy: 0.18 });
  const dissolving = !!props.leaving && phase === "pasted" && recipe.pasted.exit === "dissolve" && dissolveAllowed();
  useShellDissolve(rootRef, dissolving);

  const element = (region: string) => {
    const type = recipe.slots[region];
    if (!type) return null;
    const draw = recipe.draw[type];
    const kind = firstKind(type, draw, regions[region]);
    switch (type) {
      case "level": return <LevelPart draw={draw} kind={kind} box={box} source={source} matrix={recipe.matrix}
        sharp={recipe.style.radius === "sharp"} size={size} phase={phase} surfaceRef={surfaceRef} still={still}/>;
      case "timer": return <TimerPart draw={draw} kind={kind} text={props.timer} limited={props.limited && phase === "recording"}/>;
      case "rec": return <RecPart draw={draw}/>;
      case "mode": return <ModePart draw={draw} {...props.mode}/>;
      case "draft": return <DraftPart draw={draw} text={props.draft} captions={region === "lines"} phase={phase} still={still}
        placeholder={region === "below" || !streaming ? "" : props.draftPlaceholder}/>;
      case "limit": case "words": case "pace": case "engine": case "label": case "target":
        return <FigurePart type={type} kind={kind} box={box} figures={props.figures ?? sampleFigures()} draft={props.draft} streaming={streaming}
          limited={props.limited && phase === "recording"} label={recipe.label ?? ""} labelPlaceholder={interactive ? t("Метка") : undefined}/>;
    }
  };
  const target = (region: string) => {
    const type = recipe.slots[region];
    return {
      "data-region": region,
      "data-filled": type ?? undefined,
      "data-label": interactive?.labels[region],
      ...(interactive ? {
        tabIndex: 0, role: "button",
        "aria-label": interactive.labels[region],
        className: (interactive.selected && type === interactive.selected) || region === interactive.selectedRegion ? "ovs-sel" : undefined,
      } : {}),
    };
  };
  const R = (region: string) => {
    const attrs = target(region);
    return <div key={region} {...attrs} className={`ovs-rg ovs-rg-${region}${attrs.className ? ` ${attrs.className}` : ""}`}>{region === "edge" ? null : element(region)}</div>;
  };

  const sharp = recipe.style.radius === "sharp";
  const { processing } = recipe;
  let status: ReactNode = null;
  if (phase === "processing") {
    const sign = compact && !needsText;
    const decode = processing.draw === "decode";
    // The decoding scrambles the draft, or the status word while there is none.
    const decoded = props.draft.trim() || (typeof props.status === "string" ? props.status : "");
    const lead = processing.draw === "none" ? null : <ProcessLead draw={processing.draw} matrix={recipe.matrix} sharp={sharp}
      pace={PROCESS_PACE[processing.speed]} size={sign ? Math.round(box * (SIGN_SCALE[recipe.shell] ?? 0.62)) : Math.round(rowHeight * 0.5)}
      wide={!sign} text={decoded} chars={DECODE_CHARS[recipe.shell] ?? 26} still={still}/>;
    // Without the word on screen, the status still reaches a screen reader.
    status = <>{lead}<span className={!sign && processing.words && !decode ? "ovs-lbl" : "sr-only"}>{props.status}</span></>;
  } else if (phase === "pasted") {
    const sign = compact && !needsText;
    // An LLM warning always keeps its words: it is the reason the note needs text.
    status = <><CheckMark big={sign}/><span className={!sign && (recipe.pasted.words || needsText) ? "ovs-lbl ovs-lbl--ok" : "sr-only"}>{props.status}</span></>;
  } else if (phase === "error") {
    status = <span className="ovs-lbl ovs-lbl--err">{props.status}</span>;
  }
  // Keyed by phase: each new status enters on its own instead of swapping its words in place.
  const stl = <div key={phase} className="ovs-stl" role={phase === "recording" ? undefined : "status"}>{status}</div>;
  const { cancel } = recipe;
  const closeLook = cancel.draw === "x" ? <Icon name="x" size={14}/> : cancel.draw === "stop" ? <span className="ovs-close__stop"/> : props.close.text;
  // In the constructor the button is a target like the parts: it opens its own options.
  // A thumbnail sits inside a template's own button, so it only draws one.
  const close = still && !interactive
    ? <span className="ovs-close" data-draw={cancel.draw}>{closeLook}</span>
    : <button type="button" className={`ovs-close${interactive?.selected === "cancel" ? " ovs-sel" : ""}`} data-draw={cancel.draw}
      data-cancel={interactive ? "" : undefined} aria-label={interactive ? interactive.cancelLabel : props.close.label}
      onClick={interactive ? undefined : props.close.onClick} disabled={interactive ? false : props.close.disabled}>
      {closeLook}
    </button>;
  const at = (spot: string) => cancel.at === spot ? close : null;
  // The band continues the black of the camera notch, so it ignores the fill; the error row is an ordinary pill.
  const band = recipe.shell === "notch" && layout === "notch";
  const fill = band ? "black" : recipe.style.fill;
  const layers = <><FillLayers fill={fill} texture={recipe.style.texture} source={source} phase={phase} shown={shown} still={still}/><div className="ovs-gfx"/><div className="ovs-bb"/><div className="ovs-flash"/></>;

  // The corner radius of the shell's own skin; the split and the ring give each skin theirs.
  const radius = shellRadius(recipe, size);
  const ringShape = recipe.shell === "ring" && layout === "ring";
  const skinless = recipe.shell === "caps" || recipe.shell === "split" || recipe.shell === "ring" || band;

  let body: ReactNode;
  if (recipe.shell === "caps") {
    body = <div className="ovs-capwrap">
      <div className="ovs-chip ovs-skin" ref={surfaceRef}>{layers}<div className="ovs-main-row">{at("start")}{R("c1")}{R("c2")}{stl}{at("end")}</div></div>
      {R("lines")}
    </div>;
  } else if (recipe.shell === "split") {
    // Two capsules with a transparent gap. Outside recording the right one is drawn in
    // and the status takes the left; an error joins them into one row (see overlayScene.css).
    const capLayers = (lead: boolean) => <><FillLayers fill={fill} texture={recipe.style.texture} source={source} phase={phase} shown={shown} still={still}
      smoke={{ frame: splitSmoke, span: splitRef, lead }}/><div className="ovs-gfx"/><div className="ovs-bb"/><div className="ovs-flash"/></>;
    body = <div className="ovs-split" ref={splitRef} style={{ "--capw": `${shellHeight}px` } as CSSProperties}>
      <div className="ovs-skin ovs-cap ovs-cap-a" style={{ borderRadius: radius }}>{capLayers(true)}<div className="ovs-content">{R("dot")}</div></div>
      <div className="ovs-skin ovs-cap ovs-cap-b" style={{ borderRadius: radius }}>{capLayers(false)}<div className="ovs-content ovs-split-row">{R("start")}{R("center")}</div></div>
      {stl}{close}
    </div>;
  } else if (band) {
    // The ears sit at the window's ends, so they stay either side of the notch whatever its width.
    body = <>
      <div className="ovs-skin ovs-band" style={{ borderRadius: `0 0 ${radius}px ${radius}px` }}>{layers}</div>
      <div className="ovs-content ovs-band-row">
        <div className="ovs-ear ovs-ear-l">{R("earL")}</div>
        <div className="ovs-ear ovs-ear-r">{R("earR")}{stl}{close}</div>
      </div>
    </>;
  } else if (recipe.shell === "ring") {
    // The parts sit outside the skin: its clip-path would cut the hole's content away too.
    body = <>
      <div className="ovs-skin ovs-ring-skin" style={ringShape ? { clipPath: ringClip(shellWidth), borderRadius: "50%" } : { borderRadius: radius }}>
        {layers}
        {ringShape && <RingRim size={shellWidth} mode={phase === "recording" ? "recording" : phase === "processing" ? "processing" : "idle"}
          live={!still && shown && (phase === "recording" || phase === "processing")} source={source} pace={PROCESS_PACE[processing.speed]}
          hairline={recipe.style.stroke !== "none"} sharp={sharp}/>}
      </div>
      <div className="ovs-content ovs-ring-hole">{R("hole")}{stl}{close}</div>
    </>;
  } else {
    let content: ReactNode;
    if (recipe.shell === "pill" || recipe.shell === "island") content = <><div className="ovs-main-row">{at("start")}{R("start")}{R("center")}{R("end")}{stl}{at("end")}</div>{R("below")}</>;
    else if (recipe.shell === "card") content = <>{R("body")}<div className="ovs-foot">{at("footL")}{R("footL")}<div className="ovs-sp"/>{R("footR")}{at("footR")}</div>{stl}{at("corner")}</>;
    else if (recipe.shell === "mini") content = <div className="ovs-main-row">{at("start")}{R("start")}{R("end")}{stl}{at("end")}</div>;
    else if (recipe.shell === "notch") content = <div className="ovs-main-row">{R("earL")}{R("earR")}{stl}{close}</div>;
    else if (recipe.shell === "bead") content = <>{R("core")}{stl}{close}</>;
    else content = <>{R("top")}{R("bottom")}{stl}{close}</>;
    body = <>{layers}<div className="ovs-edgefx">{recipe.slots.edge ? element("edge") : null}</div><div className="ovs-content">{content}</div></>;
  }
  const ghosts = interactive
    ? ["below", "edge"].filter((region) => regions[region]).map((region) => {
      const attrs = target(region);
      return <div key={region} {...attrs} className={`ovs-ghost ovs-ghost-${region}${attrs.className ? ` ${attrs.className}` : ""}`}>{interactive.ghosts[region]}</div>;
    })
    : null;
  const timerless = !regionOf(recipe, "timer");

  return <div ref={rootRef} className={`ovs${still ? " ovs--still" : ""}`}
    data-shell={recipe.shell} data-radius={recipe.style.radius} data-stroke={recipe.style.stroke} data-fill={fill} data-texture={recipe.style.texture}
    data-glow={recipe.style.glow} data-font={recipe.style.font} data-motion={recipe.motion} data-phase={phase}
    data-proc-edge={recipe.processing.edge ? "1" : "0"} data-proc-speed={recipe.processing.speed} data-pasted-flash={recipe.pasted.flash ? "1" : "0"}
    data-cancel-at={cancel.at} data-cancel-show={cancel.show}
    data-band={band ? "1" : undefined} data-shown={shown ? "1" : "0"} data-exit={dissolving ? "dissolve" : undefined} data-draft={draftOpen ? "1" : "0"} data-limit={props.limited && phase === "recording" ? "1" : "0"}
    data-notimer={timerless ? "1" : "0"} data-needs-text={needsText ? "1" : "0"} data-hovered={props.hovered ? "true" : "false"}
    data-edit={interactive ? "1" : undefined} data-cancel-hidden={interactive?.hideCancel ? "1" : undefined}
    style={{ "--fs": FONT_SCALE[size], "--rowh": `${rowHeight}px`, ...(recipe.shell === "notch" ? { "--ear": `${NOTCH_EAR[size]}px` } : {}) } as CSSProperties}>
    <div className={`ovs-shell${skinless ? "" : " ovs-skin"}`} ref={recipe.shell === "caps" ? undefined : surfaceRef}
      style={{
        width: shellWidth, height: shellHeight, borderRadius: skinless ? undefined : radius,
        ...(recipe.shell === "ring" ? { "--hole": `${ringHoleSize(shellWidth)}px` } : {}),
      } as CSSProperties}>
      {recipe.shell === "caps" ? <div className="ovs-chip-radius" style={{ "--ovs-radius": `${shellRadius(recipe, size)}px` } as CSSProperties}>{body}</div> : body}
    </div>
    {ghosts}
    <div className="ovs-limitb"><span className="ovs-dot"/><span className="ovs-tt">{props.timer}</span></div>
  </div>;
}
