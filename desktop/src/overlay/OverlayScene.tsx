import { useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { Icon } from "../components/Icon";
import { CheckMark, DraftPart, LevelPart, ModePart, ProcessLead, RecPart, TimerPart } from "./sceneParts";
import type { LevelSource } from "./levelSource";
import {
  COMPACT_SHELLS, FONT_SCALE, PROCESS_PACE, SHELL_LAYOUT, SHELL_REGIONS, WINDOW_SIZE, firstKind, recipeLayout, regionOf, shellRadius, shellSize,
  type ElementType, type OverlaySize, type Recipe, type ScenePhase,
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
  /** Thumbnails: no transitions. */
  still?: boolean;
};

export function OverlayScene(props: SceneProps) {
  const { recipe, size, phase, streaming, source, interactive, still } = props;
  const shown = props.shown ?? true;
  const needsText = props.needsText ?? phase === "error";
  const draftOpen = phase === "recording" && streaming && recipe.slots.below === "draft";
  const layout = recipeLayout(recipe, draftOpen, needsText);
  const [shellWidth, shellHeight] = shellSize(recipe, size, layout, phase, shown);
  const pad = recipe.style.stroke === "rim" ? 4 : 0;
  const [designWidth, designHeight] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][size];
  const box = recipe.shell === "stack" ? designWidth - 8 - 2 * pad : Math.min(designWidth, designHeight) - 8 - 2 * pad;
  const rowHeight = WINDOW_SIZE.pill[size][1] - 8 - 2 * pad;
  const compact = COMPACT_SHELLS.includes(recipe.shell);
  const regions = SHELL_REGIONS[recipe.shell];
  const localSurface = useRef<HTMLDivElement>(null);
  const surfaceRef = props.surfaceRef ?? localSurface;

  const element = (region: string) => {
    const type = recipe.slots[region];
    if (!type) return null;
    const draw = recipe.draw[type];
    const kind = firstKind(type, draw, regions[region]);
    switch (type) {
      case "level": return <LevelPart draw={draw} kind={kind} box={box} source={source} matrix={recipe.matrix}
        sharp={recipe.style.radius === "sharp"} size={size} phase={phase} surfaceRef={surfaceRef}/>;
      case "timer": return <TimerPart draw={draw} kind={kind} text={props.timer}/>;
      case "rec": return <RecPart draw={draw}/>;
      case "mode": return <ModePart draw={draw} {...props.mode}/>;
      case "draft": return <DraftPart draw={draw} text={props.draft} captions={region === "lines"} phase={phase}
        placeholder={region === "below" || !streaming ? "" : props.draftPlaceholder}/>;
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
    const lead = processing.draw === "none" ? null : <ProcessLead draw={processing.draw} matrix={recipe.matrix} sharp={sharp}
      pace={PROCESS_PACE[processing.speed]} size={sign ? Math.round(box * (recipe.shell === "mini" ? 0.8 : 0.62)) : Math.round(rowHeight * 0.5)}/>;
    // Without the word on screen, the status still reaches a screen reader.
    status = <>{lead}<span className={!sign && processing.words ? "ovs-lbl" : "sr-only"}>{props.status}</span></>;
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
  // In the constructor the button is a target like the parts: it opens its own options.
  const close = <button type="button" className={`ovs-close${interactive?.selected === "cancel" ? " ovs-sel" : ""}`} data-draw={cancel.draw}
    data-cancel={interactive ? "" : undefined} aria-label={interactive ? interactive.cancelLabel : props.close.label}
    onClick={interactive ? undefined : props.close.onClick} disabled={interactive ? false : props.close.disabled}>
    {cancel.draw === "x" ? <Icon name="x" size={14}/> : cancel.draw === "stop" ? <span className="ovs-close__stop"/> : props.close.text}
  </button>;
  const at = (spot: string) => cancel.at === spot ? close : null;
  const layers = <><div className="ovs-gfx"/><div className="ovs-bb"/><div className="ovs-flash"/></>;

  let body: ReactNode;
  if (recipe.shell === "caps") {
    body = <div className="ovs-capwrap">
      <div className="ovs-chip ovs-skin" ref={surfaceRef}>{layers}<div className="ovs-main-row">{at("start")}{R("c1")}{R("c2")}{stl}{at("end")}</div></div>
      {R("lines")}
    </div>;
  } else {
    let content: ReactNode;
    if (recipe.shell === "pill" || recipe.shell === "island") content = <><div className="ovs-main-row">{at("start")}{R("start")}{R("center")}{R("end")}{stl}{at("end")}</div>{R("below")}</>;
    else if (recipe.shell === "card") content = <>{R("body")}<div className="ovs-foot">{at("footL")}{R("footL")}<div className="ovs-sp"/>{R("footR")}{at("footR")}</div>{stl}{at("corner")}</>;
    else if (recipe.shell === "mini") content = <div className="ovs-main-row">{at("start")}{R("start")}{R("end")}{stl}{at("end")}</div>;
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

  return <div className={`ovs${still ? " ovs--still" : ""}`}
    data-shell={recipe.shell} data-radius={recipe.style.radius} data-stroke={recipe.style.stroke} data-fill={recipe.style.fill}
    data-glow={recipe.style.glow} data-font={recipe.style.font} data-motion={recipe.motion} data-phase={phase}
    data-proc-edge={recipe.processing.edge ? "1" : "0"} data-proc-speed={recipe.processing.speed} data-pasted-flash={recipe.pasted.flash ? "1" : "0"}
    data-cancel-at={cancel.at} data-cancel-show={cancel.show}
    data-shown={shown ? "1" : "0"} data-draft={draftOpen ? "1" : "0"} data-limit={props.limited && phase === "recording" ? "1" : "0"}
    data-notimer={timerless ? "1" : "0"} data-needs-text={needsText ? "1" : "0"} data-hovered={props.hovered ? "true" : "false"}
    data-edit={interactive ? "1" : undefined} data-cancel-hidden={interactive?.hideCancel ? "1" : undefined}
    style={{ "--fs": FONT_SCALE[size], "--rowh": `${rowHeight}px` } as CSSProperties}>
    <div className={`ovs-shell${recipe.shell === "caps" ? "" : " ovs-skin"}`} ref={recipe.shell === "caps" ? undefined : surfaceRef}
      style={{ width: shellWidth, height: shellHeight, borderRadius: recipe.shell === "caps" ? undefined : shellRadius(recipe, size) }}>
      {recipe.shell === "caps" ? <div className="ovs-chip-radius" style={{ "--ovs-radius": `${shellRadius(recipe, size)}px` } as CSSProperties}>{body}</div> : body}
    </div>
    {ghosts}
    <div className="ovs-limitb"><span className="ovs-dot"/><span className="ovs-tt">{props.timer}</span></div>
  </div>;
}
