import { useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { Icon } from "../components/Icon";
import { CheckMark, DraftPart, LevelPart, ModePart, ProcessLead, RecPart, TimerPart } from "./sceneParts";
import type { LevelSource } from "./levelSource";
import {
  FONT_SCALE, SHELL_LAYOUT, SHELL_REGIONS, WINDOW_SIZE, firstKind, recipeHasMatrix, recipeLayout, regionOf, shellRadius, shellSize,
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
  close: { label: string; onClick?: () => void; disabled?: boolean };
  hovered?: boolean;
  surfaceRef?: RefObject<HTMLDivElement | null>;
  /** Constructor mode: regions become targets with labels, and the targets outside the shell appear. */
  interactive?: { labels: Record<string, string>; ghosts: Record<string, string>; selected: ElementType | null };
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
  const round = recipe.shell === "bead" || recipe.shell === "stack";
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
        className: interactive.selected && type === interactive.selected ? "ovs-sel" : undefined,
      } : {}),
    };
  };
  const R = (region: string) => {
    const attrs = target(region);
    return <div key={region} {...attrs} className={`ovs-rg ovs-rg-${region}${attrs.className ? ` ${attrs.className}` : ""}`}>{region === "edge" ? null : element(region)}</div>;
  };

  const matrix = recipeHasMatrix(recipe) ? recipe.matrix : null;
  const sharp = recipe.style.radius === "sharp";
  let status: ReactNode = null;
  if (phase === "processing") {
    status = round && !needsText
      ? matrix ? <ProcessLead matrix={matrix} size={Math.round(box * 0.62)} sharp={sharp} pixel={false}/> : <span className="ovs-comet"/>
      : <><ProcessLead matrix={matrix} size={Math.round(rowHeight * 0.5)} sharp={sharp} pixel={recipe.motion === "pixel"}/><span className="ovs-lbl">{props.status}</span></>;
  } else if (phase === "pasted") {
    status = round && !needsText ? <CheckMark big/> : <><CheckMark/><span className="ovs-lbl ovs-lbl--ok">{props.status}</span></>;
  } else if (phase === "error") {
    status = <span className="ovs-lbl ovs-lbl--err">{props.status}</span>;
  }
  const stl = <div className="ovs-stl" role={phase === "recording" ? undefined : "status"}>{status}</div>;
  const close = <button type="button" className="ovs-close" aria-label={props.close.label} onClick={props.close.onClick}
    disabled={props.close.disabled} tabIndex={interactive ? -1 : undefined}><Icon name="x" size={14}/></button>;
  const layers = <><div className="ovs-gfx"/><div className="ovs-bb"/><div className="ovs-flash"/></>;

  let body: ReactNode;
  if (recipe.shell === "caps") {
    body = <div className="ovs-capwrap">
      <div className="ovs-chip ovs-skin" ref={surfaceRef}>{layers}<div className="ovs-main-row">{R("c1")}{R("c2")}{stl}{close}</div></div>
      {R("lines")}
    </div>;
  } else {
    let content: ReactNode;
    if (recipe.shell === "pill" || recipe.shell === "island") content = <><div className="ovs-main-row">{R("start")}{R("center")}{R("end")}{stl}{close}</div>{R("below")}</>;
    else if (recipe.shell === "card") content = <>{R("body")}<div className="ovs-foot">{R("footL")}<div className="ovs-sp"/>{R("footR")}{close}</div>{stl}</>;
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
    data-shown={shown ? "1" : "0"} data-draft={draftOpen ? "1" : "0"} data-limit={props.limited && phase === "recording" ? "1" : "0"}
    data-notimer={timerless ? "1" : "0"} data-needs-text={needsText ? "1" : "0"} data-hovered={props.hovered ? "true" : "false"}
    style={{ "--fs": FONT_SCALE[size], "--rowh": `${rowHeight}px` } as CSSProperties}>
    <div className={`ovs-shell${recipe.shell === "caps" ? "" : " ovs-skin"}`} ref={recipe.shell === "caps" ? undefined : surfaceRef}
      style={{ width: shellWidth, height: shellHeight, borderRadius: recipe.shell === "caps" ? undefined : shellRadius(recipe, size) }}>
      {recipe.shell === "caps" ? <div className="ovs-chip-radius" style={{ "--ovs-radius": `${shellRadius(recipe, size)}px` } as CSSProperties}>{body}</div> : body}
    </div>
    {ghosts}
    <div className="ovs-limitb"><span className="ovs-dot"/><span className="ovs-tt">{props.timer}</span></div>
  </div>;
}
