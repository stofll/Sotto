import { useEffect, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { Hint } from "../../components/Hint";
import { Icon } from "../../components/Icon";
import { NumberField } from "../../components/NumberField";
import { Segmented } from "../../components/Shell";
import { t } from "../../i18n";
import { MATRIX_DENSITIES, MATRIX_PROCESS, MATRIX_SPEECH } from "../../overlay/dotMatrix";
import { OverlayMatrix } from "../../overlay/OverlayMatrix";
import { overlayPalette } from "../../overlay/overlayPalette";
import { OVERLAY_ANCHORS, type OverlayPreferences } from "../../overlay/overlayPreferences";
import {
  CANCEL_SPOTS, DRAWINGS, MOTIONS, SHELLS, cancelDraws, cancelShows, SHELL_LAYOUT, STYLE_OPTIONS, WINDOW_SIZE, fitsShell, regionOf, shellRadius,
  type ElementType, type Recipe, type RecipeMatrix, type RecipeStyle, type RegionKind, type Shell, type UserTemplate,
} from "../../overlay/overlayRecipe";
import { DraftPart, LevelPart, ModePart, RecPart, TimerPart } from "../../overlay/sceneParts";
import { cancelDrawNames, cancelShowNames, cancelSpotNames, drawNames, elementNames, elementNotes, kindPlaces, motionNames, processPatternNames, regionNames, shellNames, speechPatternNames, styleNames } from "./labels";
import type { OverlayLook } from "./overlayDraft";
import { drawingKinds } from "./recipeEdits";
import { simulatedVoice } from "./simulatedVoice";
import { MyTemplates, SystemTemplates } from "./TemplateTiles";
import { SYSTEM_TEMPLATES, type SystemTemplate } from "./templates";

export type LibraryTab = "templates" | "build" | "style" | "place";
const LIBRARY_ORDER: ElementType[] = ["level", "timer", "rec", "draft", "mode"];
const ALL_SYSTEM = Object.keys(SYSTEM_TEMPLATES) as SystemTemplate[];

/** The shape a part is previewed in: the place it most naturally goes. */
function previewKind(type: ElementType, draw: string): RegionKind {
  const kinds = DRAWINGS[type][draw] ?? [];
  if (type === "level" && kinds.includes("wide")) return "wide";
  if (kinds.includes("text")) return "text";
  if (kinds.includes("edge")) return "edge";
  if (kinds.includes("square") && (draw === "ring" || draw === "orb")) return "square";
  return kinds.includes("small") ? "small" : kinds[0] ?? "small";
}

/** A part drawn alive, the way it will look in the shell with the current style. */
export function PartPreview({ type, draw, recipe }: { type: ElementType; draw: string; recipe: Recipe }) {
  const kind = previewKind(type, draw);
  const sample = t("Коллеги, по итогам созвона: релиз переносим на четверг");
  let part;
  switch (type) {
    case "level": part = <LevelPart draw={draw} kind={kind} box={44} source={simulatedVoice} matrix={recipe.matrix} sharp={recipe.style.radius === "sharp"} size="m" phase="recording"/>; break;
    case "timer": part = <TimerPart draw={draw} kind={kind} text="00:07"/>; break;
    case "rec": part = <RecPart draw={draw}/>; break;
    case "mode": part = <ModePart draw={draw} full="RU · large-v3" short="RU"/>; break;
    case "draft": part = <DraftPart draw={draw} text={sample} captions={false} phase="recording" placeholder=""/>; break;
  }
  return <span className="ovs ove-pv-root" data-radius={recipe.style.radius} data-fill={recipe.style.fill} data-stroke={recipe.style.stroke}
    data-font={recipe.style.font} data-phase="recording" style={{ "--fs": 0.92, "--rowh": "36px" } as CSSProperties} aria-hidden="true">
    <span className={`ove-pv ove-pv--${kind}`}>{part}</span>
  </span>;
}

type Props = {
  tab: LibraryTab;
  onTab: (tab: LibraryTab) => void;
  recipe: Recipe;
  preferences: OverlayPreferences;
  onRecipe: (recipe: Recipe, message?: string) => void;
  onShell: (shell: Shell) => void;
  onAddPart: (type: ElementType, draw: string) => void;
  onPartPointerDown: (event: ReactPointerEvent, type: ElementType, draw: string) => void;
  onLook: (look: Partial<OverlayLook>) => void;
  onApplySystem: (key: SystemTemplate) => void;
  onApplyMine: (template: UserTemplate) => void;
  onEditMine: (template: UserTemplate) => void;
  onTemplates: (templates: UserTemplate[], saved?: UserTemplate) => void;
};

export function EditorLibrary(props: Props) {
  const { tab, onTab, recipe, preferences } = props;
  return <div className="card ove-lib" style={overlayPalette(preferences)}>
    <Segmented value={tab} onChange={(value) => onTab(value as LibraryTab)} options={[
      { value: "templates", label: t("Шаблоны") }, { value: "build", label: t("Сборка") },
      { value: "style", label: t("Стиль") }, { value: "place", label: t("Место") },
    ]}/>
    {tab === "templates" && <>
      <section className="ove-sec">
        <header className="ove-sec__head"><b>{t("Мои шаблоны")}</b><span>{t("со своим цветом и размером")}</span></header>
        <MyTemplates layout="grid" recipe={recipe} preferences={preferences} onApply={props.onApplyMine} onEdit={props.onEditMine} onChange={props.onTemplates}/>
      </section>
      <section className="ove-sec">
        <header className="ove-sec__head"><b>{t("Шаблоны Sotto")}</b><span>{t("цвет, размер и место сохранятся")}</span></header>
        <SystemTemplates keys={ALL_SYSTEM} recipe={recipe} preferences={preferences} onApply={props.onApplySystem}/>
      </section>
    </>}
    {tab === "build" && <BuildTab {...props}/>}
    {tab === "style" && <StyleTab {...props}/>}
    {tab === "place" && <PlaceTab {...props}/>}
  </div>;
}

function BuildTab({ recipe, preferences, onShell, onAddPart, onPartPointerDown, onRecipe }: Props) {
  const shells = shellNames(), elements = elementNames(), notes = elementNotes(), draws = drawNames(), regions = regionNames(), places = kindPlaces();
  const [windowWidth, windowHeight] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][preferences.size];
  return <>
    <section className="ove-sec">
      <header className="ove-sec__head"><b>{t("Корпус")}</b><span>{t("окно {p0}×{p1}", { p0: windowWidth, p1: windowHeight })}</span></header>
      <div className="ove-shells">
        {SHELLS.map((shell) => {
          const [width, height] = WINDOW_SIZE[SHELL_LAYOUT[shell]][preferences.size];
          const scale = Math.min(64 / width, 32 / height);
          const radius = Math.min(shellRadius({ ...recipe, shell }, preferences.size), (Math.min(width, height) - 8) / 2) * scale;
          return <button key={shell} type="button" className="ove-shell" aria-pressed={recipe.shell === shell} onClick={() => onShell(shell)}>
            <span className={`ove-sil ove-sil--${shell}`}>
              {shell === "caps" ? <><i/><i/><i/></> : <i style={{ width: (width - 8) * scale, height: (height - 8) * scale, borderRadius: radius }}/>}
            </span>
            <b>{shells[shell]}</b><span>{width}×{height}</span>
          </button>;
        })}
      </div>
    </section>
    {LIBRARY_ORDER.map((type) => {
      const where = regionOf(recipe, type);
      return <section className="ove-sec" key={type}>
        <header className="ove-sec__head"><b>{elements[type]}</b><span>{notes[type]}</span></header>
        <div className="ove-parts">
          {Object.keys(DRAWINGS[type]).map((draw) => {
            const fits = fitsShell(type, draw, recipe.shell);
            const used = !!where && recipe.draw[type] === draw;
            const hint = fits
              ? used ? t("Уже в макете. Перетащите, чтобы переставить") : t("Нажмите, чтобы добавить, или перетащите в область")
              : t("В корпус «{p0}» не помещается. Подходит: {p1}", { p0: shells[recipe.shell], p1: drawingKinds(type, draw).map((kind) => places[kind]).join(", ") });
            const wide = type === "draft" || draw === "scope" || draw === "beam";
            return <Hint key={draw} asChild text={hint}>
              <button type="button" className={`ove-part${wide ? " ove-part--wide" : ""}`} aria-pressed={used} aria-disabled={!fits}
                aria-label={`${elements[type]}: ${draws[type][draw]}. ${hint}`}
                onClick={() => onAddPart(type, draw)} onPointerDown={(event) => fits && onPartPointerDown(event, type, draw)}>
                <span className="ove-part__pv"><PartPreview type={type} draw={draw} recipe={recipe}/></span>
                <span className="ove-part__meta"><b>{draws[type][draw]}</b>
                  <span>{used && where ? t("в «{p0}»", { p0: regions[where] }) : fits ? "" : t("не сюда")}</span></span>
              </button>
            </Hint>;
          })}
        </div>
        {type === "level" && where && recipe.draw.level === "matrix" && <MatrixOptions recipe={recipe} onRecipe={onRecipe}/>}
      </section>;
    })}
    <section className="ove-sec">
      <header className="ove-sec__head"><b>{t("Отмена")}</b><span>{t("есть всегда, её можно переставить")}</span></header>
      <CancelOptions recipe={recipe} onRecipe={onRecipe}/>
    </section>
  </>;
}

/** The cancel button's look, place and visibility. It has no "remove": see `CANCEL_SPOTS`. */
export function CancelOptions({ recipe, onRecipe }: { recipe: Recipe; onRecipe: (recipe: Recipe, message: string) => void }) {
  const draws = cancelDrawNames(), spots = cancelSpotNames(), shows = cancelShowNames();
  const set = (patch: Partial<Recipe["cancel"]>, message: string) => onRecipe({ ...recipe, cancel: { ...recipe.cancel, ...patch } }, message);
  const spotList = CANCEL_SPOTS[recipe.shell], showList = cancelShows(recipe.shell);
  return <div className="ove-cancel">
    <div className="ove-opts">
      {cancelDraws(recipe.shell).map((draw) => <button key={draw} type="button" className="ove-opt" aria-pressed={recipe.cancel.draw === draw}
        onClick={() => set({ draw }, t("Отмена: «{p0}»", { p0: draws[draw] }))}>
        <span className="ove-cancel__pv">{draw === "x" ? <Icon name="x" size={12}/> : draw === "stop" ? <i/> : t("Отмена")}</span>
        <span>{draws[draw]}</span>
      </button>)}
    </div>
    {spotList.length > 1 && <div className="ove-row">
      <span className="ove-cancel__label">{t("Где")}</span>
      <Segmented value={recipe.cancel.at} onChange={(at) => set({ at }, t("Отмена: {p0}", { p0: spots[at] }))}
        options={spotList.map((spot) => ({ value: spot, label: spots[spot] }))}/>
    </div>}
    <div className="ove-row">
      <span className="ove-cancel__label">{t("Видна")}</span>
      {showList.length > 1
        ? <Segmented value={recipe.cancel.show} onChange={(show) => set({ show: show as Recipe["cancel"]["show"] }, t("Отмена: {p0}", { p0: shows[show as Recipe["cancel"]["show"]].toLowerCase() }))}
          options={showList.map((show) => ({ value: show, label: shows[show] }))}/>
        : <span className="ove-cancel__note">{t("при наведении: в этом корпусе кнопка лежит поверх деталей")}</span>}
    </div>
  </div>;
}

/** Patterns and density for the dot matrix, each drawn alive. */
export function MatrixOptions({ recipe, onRecipe }: { recipe: Recipe; onRecipe: Props["onRecipe"] }) {
  const speech = speechPatternNames(), process = processPatternNames();
  const set = (patch: Partial<RecipeMatrix>, message: string) => onRecipe({ ...recipe, matrix: { ...recipe.matrix, ...patch } }, message);
  const sharp = recipe.style.radius === "sharp";
  const tile = (key: string, pressed: boolean, label: string, note: string, onClick: () => void, preview: RecipeMatrix, mode: "speech" | "process") =>
    <button key={key} type="button" className="ove-mxo" aria-pressed={pressed} onClick={onClick}>
      <span className="ove-mxo__pv"><OverlayMatrix mode={mode} size={40} sharp={sharp} source={simulatedVoice} {...preview}/></span>
      <b>{label}</b><span>{note}</span>
    </button>;
  return <div className="ove-matrix" id="overlay-matrix-options">
    <header className="ove-sec__head"><b>{t("Матрица")}</b><span>{t("узор речи, обработки и плотность")}</span></header>
    <p className="ove-matrix__title">{t("Когда вы говорите")}</p>
    <div className="ove-mxopts">{MATRIX_SPEECH.map((value) => tile(value, recipe.matrix.speech === value, speech[value][0], speech[value][1],
      () => set({ speech: value }, t("Матрица: «{p0}»", { p0: speech[value][0] })), { ...recipe.matrix, speech: value }, "speech"))}</div>
    <p className="ove-matrix__title">{t("Пока текст обрабатывается")}</p>
    <div className="ove-mxopts">{MATRIX_PROCESS.map((value) => tile(value, recipe.matrix.process === value, process[value][0], process[value][1],
      () => set({ process: value }, t("Обработка: «{p0}». Фаза «Обработка» под макетом покажет её в корпусе", { p0: process[value][0] })), { ...recipe.matrix, process: value }, "process"))}</div>
    <p className="ove-matrix__title">{t("Плотность")}</p>
    <div className="ove-mxopts ove-mxopts--three">{MATRIX_DENSITIES.map((value) => tile(String(value), recipe.matrix.density === value, `${value} × ${value}`,
      value === 5 ? t("крупно") : value === 7 ? t("средне") : t("плотно"),
      () => set({ density: value }, t("Матрица: плотность {p0} × {p0}", { p0: value })), { ...recipe.matrix, density: value }, "speech"))}</div>
  </div>;
}

function StyleTab({ recipe, preferences, onRecipe, onLook }: Props) {
  const names = styleNames(), motions = motionNames();
  const paletteOptions: Array<{ value: OverlayPreferences["palette"]; label: string }> = [
    { value: "graphite", label: t("Графит") }, { value: "copper", label: t("Медь") },
    { value: "lagoon", label: t("Лагуна") }, { value: "violet", label: t("Фиолетовый") },
    { value: "custom", label: t("Своя палитра") },
  ];
  return <>
    <section className="ove-sec">
      <header className="ove-sec__head"><b>{t("Цвет и размер")}</b></header>
      <div className="ove-row">
        <div className="overlay-swatches">
          {paletteOptions.map(({ value, label }) => <Hint key={value} text={label}>
            <button type="button" className={`overlay-swatch${value === "custom" ? " overlay-swatch--custom" : ""}`}
              aria-label={label} aria-pressed={preferences.palette === value}
              style={overlayPalette({ ...preferences, palette: value })} onClick={() => onLook({ palette: value })}/>
          </Hint>)}
        </div>
        <Segmented value={preferences.size} onChange={(size) => onLook({ size: size as OverlayPreferences["size"] })}
          options={[{ value: "s", label: "S" }, { value: "m", label: "M" }, { value: "l", label: "L" }]}/>
      </div>
    </section>
    {(Object.keys(STYLE_OPTIONS) as (keyof RecipeStyle)[]).map((key) => {
      const [title, options] = names[key];
      return <section className="ove-sec" key={key}>
        <header className="ove-sec__head"><b>{title}</b></header>
        <div className="ove-opts">
          {(STYLE_OPTIONS[key] as readonly string[]).map((value) => <button key={value} type="button" className="ove-opt"
            aria-pressed={recipe.style[key] === value}
            onClick={() => onRecipe({ ...recipe, style: { ...recipe.style, [key]: value } }, `${title}: ${(options as Record<string, string>)[value].toLowerCase()}`)}>
            <span className={`ove-mini ove-mini--${key}`} data-value={value}>{key === "font" ? "07" : null}</span>
            <span>{(options as Record<string, string>)[value]}</span>
          </button>)}
        </div>
      </section>;
    })}
    <section className="ove-sec">
      <header className="ove-sec__head"><b>{t("Движение")}</b><span>{t("как корпус переходит между состояниями")}</span></header>
      <div className="ove-motions">
        {MOTIONS.map((motion) => <button key={motion} type="button" className="ove-motion" aria-pressed={recipe.motion === motion}
          onClick={() => onRecipe({ ...recipe, motion }, t("Движение: {p0}", { p0: motions[motion][0].toLowerCase() }))}>
          <span className="ove-motion__demo" data-motion={motion}><i/></span>
          <b>{motions[motion][0]}</b><span>{motions[motion][1]}</span>
        </button>)}
      </div>
    </section>
  </>;
}

function PlaceTab({ preferences, onLook }: Props) {
  const [offset, setOffset] = useState(preferences.edge_offset);
  useEffect(() => setOffset(preferences.edge_offset), [preferences.edge_offset]);
  const commitOffset = (value: number) => {
    if (Number.isInteger(value) && value >= 0 && value <= 512) onLook({ edge_offset: value });
    else setOffset(preferences.edge_offset);
  };
  const labels = [t("Сверху слева"), t("Сверху по центру"), t("Сверху справа"),
    t("Слева по центру"), t("По центру"), t("Справа по центру"),
    t("Снизу слева"), t("Снизу по центру"), t("Снизу справа")];
  return <section className="ove-sec">
    <header className="ove-sec__head"><b>{t("Положение на экране")}</b><span>{t("шаблоны его не меняют")}</span></header>
    <div className="ove-anchors">
      {OVERLAY_ANCHORS.map((anchor, index) => <Hint key={anchor} asChild text={labels[index]}>
        <button type="button" aria-label={labels[index]} aria-pressed={preferences.anchor === anchor} onClick={() => onLook({ anchor })}><i/></button>
      </Hint>)}
    </div>
    <div className="ove-row">
      <label className="set-label" htmlFor="ove-offset">{t("Отступ от края")}</label>
      <NumberField id="ove-offset" min={0} max={512} step={1} value={offset} disabled={preferences.anchor === "center"}
        onValueChange={(value) => setOffset(Number(value))} onStepCommit={(value) => commitOffset(Number(value))}
        onBlur={() => commitOffset(offset)}/>
    </div>
  </section>;
}

