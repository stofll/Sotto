import { useEffect, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { Hint } from "../../components/Hint";
import { Icon } from "../../components/Icon";
import { Segmented } from "../../components/Shell";
import { t } from "../../i18n";
import { MATRIX_DENSITIES, MATRIX_PROCESS, MATRIX_SPEECH } from "../../overlay/dotMatrix";
import { OverlayMatrix } from "../../overlay/OverlayMatrix";
import { overlayPalette } from "../../overlay/overlayPalette";
import type { OverlayPreferences } from "../../overlay/overlayPreferences";
import {
  CANCEL_DRAWS, CANCEL_SHOWS, CANCEL_SPOTS, COMPACT_SHELLS, DRAWINGS, PASTED_HOLDS, MOTIONS, PROCESS_DRAWS, PROCESS_PACE, PROCESS_SPEEDS, SHELLS, cancelDraws, cancelShows, SHELL_LAYOUT, STYLE_OPTIONS, WINDOW_SIZE, fitsShell, regionOf, shellRadius,
  recipeHasMatrix, type ElementType, type ProcessDraw, type PastedHold, type ProcessSpeed, type Recipe, type RecipePasted, type RecipeProcessing, type RecipeMatrix, type RecipeStyle, type RegionKind, type Shell, type UserTemplate,
} from "../../overlay/overlayRecipe";
import { DraftPart, LevelPart, ModePart, ProcessLead, RecPart, TimerPart } from "../../overlay/sceneParts";
import { cancelDrawNames, cancelShowNames, cancelSpotNames, drawNames, elementNames, elementNotes, kindPlaces, motionNames, pastedHoldNames, processNames, processPatternNames, processSpeedNames, regionNames, shellNames, speechPatternNames, styleNames } from "./labels";
import type { OverlayLook } from "./overlayDraft";
import { drawingKinds, PART_ORDER } from "./recipeEdits";
import { simulatedVoice, type PhaseMode } from "./simulatedVoice";
import { PaletteTone } from "./PaletteTone";
import { MyTemplates, SystemTemplates } from "./TemplateTiles";
import { SYSTEM_TEMPLATES, type SystemTemplate } from "./templates";

export type LibraryTab = "templates" | "build" | "style";
const ALL_SYSTEM = Object.keys(SYSTEM_TEMPLATES) as SystemTemplate[];

/** The shape a part is previewed in: the place it most naturally goes. */
function previewKind(type: ElementType, draw: string): RegionKind {
  const kinds = DRAWINGS[type][draw] ?? [];
  if (kinds.includes("square") && (draw === "ring" || draw === "orb")) return "square";
  if (type === "level" && kinds.includes("wide")) return "wide";
  if (kinds.includes("text")) return "text";
  if (kinds.includes("edge")) return "edge";
  return kinds.includes("small") ? "small" : kinds[0] ?? "small";
}

/** A processing sign, drawn the way the pill row shows it, at the recipe's speed. */
function ProcessPreview({ draw, recipe }: { draw: ProcessDraw; recipe: Recipe }) {
  const speed = recipe.processing.speed;
  return <span className="ovs ove-pv-root" data-radius={recipe.style.radius} data-fill={recipe.style.fill} data-motion={recipe.motion}
    data-phase="processing" data-proc-speed={speed} style={{ "--fs": 0.92 } as CSSProperties} aria-hidden="true">
    <span className="ove-pv ove-pv--small">{draw === "none" ? null
      : <ProcessLead draw={draw} matrix={recipe.matrix} size={26} sharp={recipe.style.radius === "sharp"} pace={PROCESS_PACE[speed]}/>}</span>
  </span>;
}

/** Everything the overlay shows while processing: the sign, the light on the stroke, the word and the pace. */
function ProcessingOptions({ recipe, onRecipe }: { recipe: Recipe; onRecipe: Props["onRecipe"] }) {
  const names = processNames(), speeds = processSpeedNames(), { processing } = recipe;
  const set = (patch: Partial<RecipeProcessing>, message: string) =>
    onRecipe({ ...recipe, processing: { ...processing, ...patch } }, message);
  const noStroke = recipe.style.stroke === "none", compact = COMPACT_SHELLS.includes(recipe.shell);
  return <div className="ove-cancel">
    <div className="ove-parts">
      {PROCESS_DRAWS.map((draw) => <button key={draw} type="button" className="ove-part ove-part--pick" aria-pressed={processing.draw === draw}
        onClick={() => set({ draw }, t("Обработка: «{p0}»", { p0: names[draw] }))}>
        <span className="ove-part__pv"><ProcessPreview draw={draw} recipe={recipe}/></span>
        <span className="ove-part__meta"><b>{names[draw]}</b></span>
      </button>)}
    </div>
    {/* One row: the three settings are short, and each explains itself behind its info icon. */}
    <div className="ove-procrow">
      <span className="ove-procrow__item">
        <label className="checkbox-row ove-check">
          <input className="checkbox" type="checkbox" checked={processing.edge && !noStroke} disabled={noStroke}
            onChange={(event) => set({ edge: event.target.checked }, event.target.checked ? t("Обработка: огонёк по обводке") : t("Обработка: без огонька"))}/>
          {t("Огонёк по обводке")}
        </label>
        <Hint text={t("Пока текст обрабатывается, по обводке корпуса бежит светлый блик.") + (noStroke ? " " + t("У этого корпуса нет обводки: включите её во вкладке «Стиль».") : "")}/>
      </span>
      <span className="ove-procrow__item">
        <label className="checkbox-row ove-check">
          <input className="checkbox" type="checkbox" checked={processing.words && !compact} disabled={compact}
            onChange={(event) => set({ words: event.target.checked }, event.target.checked ? t("Обработка: с подписью") : t("Обработка: без подписи"))}/>
          {t("Подпись")}
        </label>
        <Hint text={compact ? t("В маленьком корпусе места для слов нет: показывается только знак.")
          : t("Рядом со знаком пишется «Обрабатываю», а при долгой обработке ещё и сколько секунд она идёт.")}/>
      </span>
      <span className="ove-procrow__item">
        <span className="ove-check">{t("Темп")}</span>
        <Hint text={t("Скорость всей анимации обработки: знака, огонька и узора матрицы.")}/>
        <Segmented value={processing.speed} onChange={(speed) => set({ speed: speed as ProcessSpeed }, t("Обработка: {p0}", { p0: speeds[speed as ProcessSpeed].toLowerCase() }))}
          options={PROCESS_SPEEDS.map((speed) => ({ value: speed, label: speeds[speed] }))}/>
      </span>
    </div>
  </div>;
}

/** What a folded group holds, in a line. */
function groupSummaries(recipe: Recipe): Record<GroupId, string> {
  const elements = elementNames(), { processing, pasted, cancel } = recipe;
  const placed = PART_ORDER.filter((type) => regionOf(recipe, type)).map((type) => elements[type]);
  const edge = processing.edge && recipe.style.stroke !== "none", compact = COMPACT_SHELLS.includes(recipe.shell);
  const flash = pasted.flash && recipe.motion !== "quiet";
  return {
    record: placed.length ? placed.join(", ") : t("пусто"),
    process: [processNames()[processing.draw], edge && t("огонёк"), processing.words && !compact && t("подпись"),
      processSpeedNames()[processing.speed].toLowerCase()].filter(Boolean).join(" · "),
    pasted: [flash ? t("вспышка") : t("без вспышки"), pasted.words && !compact ? t("с подписью") : t("только галочка"),
      pastedHoldNames()[pasted.hold].toLowerCase()].join(" · "),
    cancel: [cancelDrawNames()[cancel.draw], cancelSpotNames()[cancel.at], cancelShowNames()[cancel.show].toLowerCase()].join(" · "),
  };
}

/** The "inserted" note: its flash, its words, and how long it stays. */
function PastedOptions({ recipe, onRecipe }: { recipe: Recipe; onRecipe: Props["onRecipe"] }) {
  const holds = pastedHoldNames(), { pasted } = recipe;
  const set = (patch: Partial<RecipePasted>, message: string) => onRecipe({ ...recipe, pasted: { ...pasted, ...patch } }, message);
  const quiet = recipe.motion === "quiet", compact = COMPACT_SHELLS.includes(recipe.shell);
  return <div className="ove-procrow">
    <span className="ove-procrow__item">
      <label className="checkbox-row ove-check">
        <input className="checkbox" type="checkbox" checked={pasted.flash && !quiet} disabled={quiet}
          onChange={(event) => set({ flash: event.target.checked }, event.target.checked ? t("Вставка: со вспышкой") : t("Вставка: без вспышки"))}/>
        {t("Вспышка")}
      </label>
      <Hint text={quiet ? t("При движении «{p0}» вспышки нет: его можно сменить во вкладке «Стиль».", { p0: motionNames().quiet[0] })
        : t("Когда текст вставлен, контур корпуса коротко вспыхивает зелёным.")}/>
    </span>
    <span className="ove-procrow__item">
      <label className="checkbox-row ove-check">
        <input className="checkbox" type="checkbox" checked={pasted.words && !compact} disabled={compact}
          onChange={(event) => set({ words: event.target.checked }, event.target.checked ? t("Вставка: с подписью") : t("Вставка: только галочка"))}/>
        {t("Подпись")}
      </label>
      <Hint text={compact ? t("В маленьком корпусе места для слов нет: показывается только знак.")
        : t("Рядом с галочкой пишется, сколько символов вставлено. Предупреждение LLM видно всегда.")}/>
    </span>
    <span className="ove-procrow__item">
      <span className="ove-check">{t("Держится")}</span>
      <Hint text={t("Сколько заметка остаётся на экране. С предупреждением LLM она держится не меньше обычного, чтобы его успели прочитать.")}/>
      <Segmented value={pasted.hold} onChange={(hold) => set({ hold: hold as PastedHold }, t("Вставка: держится {p0}", { p0: holds[hold as PastedHold].toLowerCase() }))}
        options={PASTED_HOLDS.map((hold) => ({ value: hold, label: holds[hold] }))}/>
    </span>
  </div>;
}

/** A library section's title; its explanation waits behind the info icon, as in Settings. */
function SectionHead({ title, hint }: { title: string; hint: string }) {
  return <header className="ove-sec__head"><b className="ove-sec__title">{title}<Hint text={hint}/></b></header>;
}

/** Parts whose preview needs a whole row of the parts grid: only the draft's text is wider than a tile. */
export const isWidePart = (type: ElementType) => type === "draft";

/** A part drawn alive, the way it will look in the shell with the current style. The stroke is the
 *  shell's outline, so it stays on the shell tiles rather than framing every part. */
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
  return <span className="ovs ove-pv-root" data-radius={recipe.style.radius} data-fill={recipe.style.fill}
    data-font={recipe.style.font} data-phase="recording" style={{ "--fs": 0.92, "--rowh": "36px" } as CSSProperties} aria-hidden="true">
    <span className={`ove-pv ove-pv--${kind}`}>{part}</span>
  </span>;
}

type Props = {
  tab: LibraryTab;
  /** A group of the Build tab is being edited: show its phase under the stage. */
  onPhase?: (phase: PhaseMode) => void;
  /** The phase shown under the stage: picking one there opens its group. */
  phase?: PhaseMode;
  onTab: (tab: LibraryTab) => void;
  recipe: Recipe;
  preferences: OverlayPreferences;
  onRecipe: (recipe: Recipe, message?: string) => void;
  onShell: (shell: Shell) => void;
  onAddPart: (type: ElementType, draw: string) => void;
  onPartPointerDown: (event: ReactPointerEvent, type: ElementType, draw: string) => void;
  onLook: (look: Partial<OverlayLook>) => void;
  /** A slider step: shown at once, saved by `onLookCommit` when the slider is released. */
  onLookPreview: (look: Partial<OverlayLook>) => void;
  onLookCommit: (look: Partial<OverlayLook>) => void;
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
      { value: "style", label: t("Стиль") },
    ]}/>
    {tab === "templates" && <>
      <section className="ove-sec">
        <SectionHead title={t("Мои шаблоны")} hint={t("Шаблоны хранят свой цвет и размер")}/>
        <MyTemplates layout="grid" recipe={recipe} preferences={preferences} onApply={props.onApplyMine} onEdit={props.onEditMine} onChange={props.onTemplates}/>
      </section>
      <section className="ove-sec">
        <SectionHead title={t("Шаблоны Sotto")} hint={t("Цвет, размер и место оверлея сохранятся")}/>
        <SystemTemplates keys={ALL_SYSTEM} recipe={recipe} preferences={preferences} onApply={props.onApplySystem}/>
      </section>
    </>}
    {tab === "build" && <BuildTab {...props}/>}
    {tab === "style" && <StyleTab {...props}/>}
  </div>;
}

type GroupId = "record" | "process" | "pasted" | "cancel";
const GROUP_OF_PHASE: Partial<Record<PhaseMode, GroupId>> = {
  recording: "record", streaming: "record", limit: "record", processing: "process", pasted: "pasted",
};
const OPEN_GROUPS_KEY = "sotto.overlayEditor.openGroups";
const DEFAULT_OPEN: Record<GroupId, boolean> = { record: true, process: false, pasted: false, cancel: false };

/** Which groups are open, remembered across visits like the sidebar's groups.
 *  A phase picked under the stage opens its group and brings it into view. */
function useOpenGroups(phase: PhaseMode | undefined) {
  const [open, setOpen] = useState<Record<GroupId, boolean>>(() => {
    try {
      const saved = window.localStorage.getItem(OPEN_GROUPS_KEY);
      if (saved) return { ...DEFAULT_OPEN, ...JSON.parse(saved) as Partial<Record<GroupId, boolean>> };
    } catch { /* optional storage */ }
    return DEFAULT_OPEN;
  });
  const openRef = useRef(open);
  openRef.current = open;
  const set = (id: GroupId, value: boolean) => setOpen((current) => {
    const next = { ...current, [id]: value };
    try { window.localStorage.setItem(OPEN_GROUPS_KEY, JSON.stringify(next)); } catch { /* optional storage */ }
    return next;
  });
  useEffect(() => {
    const id = phase && GROUP_OF_PHASE[phase];
    if (!id || openRef.current[id]) return;
    set(id, true);
    requestAnimationFrame(() => document.getElementById(`overlay-group-${id}`)?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  }, [phase]);
  return [open, set] as const;
}

/**
 * A phase of the dictation in the Build tab. It folds away, showing a summary
 * instead, and a folded group draws nothing, so its live previews cost nothing.
 * Working in a group shows its phase under the stage; so does opening it, but
 * not the header click itself, which would otherwise open the group through
 * the phase and fold it again through the click.
 */
function Group({ id, title, hint, summary, open, onOpen, phase, onPhase, children }: {
  id: GroupId; title: string; hint: string; summary: string; open: boolean; onOpen: (open: boolean) => void;
  phase?: PhaseMode; onPhase?: (phase: PhaseMode) => void; children: ReactNode;
}) {
  const show = (event: { target: EventTarget }) => {
    if (phase && onPhase && !(event.target instanceof Element && event.target.closest(".ove-group__head"))) onPhase(phase);
  };
  return <div className="ove-group" id={`overlay-group-${id}`} onPointerDownCapture={show} onFocusCapture={show}>
    <div className="ove-group__head">
      <button type="button" className="ove-group__toggle" aria-expanded={open}
        onClick={() => { onOpen(!open); if (!open && phase) onPhase?.(phase); }}>
        <span className="fold__chev" data-open={open ? "true" : "false"}><Icon name="chev-right" size={12}/></span>
        <span className="ove-group__title">{title}</span>
      </button>
      <Hint text={hint}/>
      {!open && <span className="ove-group__summary">{summary}</span>}
    </div>
    {open && children}
  </div>;
}

function BuildTab({ recipe, preferences, onShell, onAddPart, onPartPointerDown, onRecipe, onPhase, phase }: Props) {
  const shells = shellNames(), elements = elementNames(), notes = elementNotes(), draws = drawNames(), regions = regionNames(), places = kindPlaces();
  const [open, setOpen] = useOpenGroups(phase);
  const group = (id: GroupId) => ({ id, open: open[id], onOpen: (value: boolean) => setOpen(id, value), onPhase });
  const summaries = groupSummaries(recipe);
  const [windowWidth, windowHeight] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][preferences.size];
  return <>
    <section className="ove-sec">
      <SectionHead title={t("Корпус")} hint={t("Окно оверлея с этим корпусом: {p0}×{p1}", { p0: windowWidth, p1: windowHeight })}/>
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
    <Group {...group("record")} title={t("Запись")} summary={summaries.record} phase="recording"
      hint={t("Детали, которые видны, пока вы говорите. Нажмите деталь или перетащите её в область макета")}>
    {PART_ORDER.map((type) => {
      const where = regionOf(recipe, type);
      return <section className="ove-sec" key={type}>
        <SectionHead title={elements[type]} hint={notes[type]}/>
        <div className="ove-parts">
          {Object.keys(DRAWINGS[type]).map((draw) => {
            const fits = fitsShell(type, draw, recipe.shell);
            const used = !!where && recipe.draw[type] === draw;
            const hint = fits
              ? used ? t("Уже в макете. Перетащите, чтобы переставить") : t("Нажмите, чтобы добавить, или перетащите в область")
              : t("В корпус «{p0}» не помещается. Подходит: {p1}", { p0: shells[recipe.shell], p1: drawingKinds(type, draw).map((kind) => places[kind]).join(", ") });
            return <Hint key={draw} asChild text={hint}>
              <button type="button" className={`ove-part${isWidePart(type) ? " ove-part--wide" : ""}`} aria-pressed={used} aria-disabled={!fits}
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
    </Group>
    <Group {...group("process")} title={t("Обработка")} summary={summaries.process} phase="processing"
      hint={t("Что оверлей показывает, пока распознаёт и обрабатывает текст")}>
      <section className="ove-sec">
        <ProcessingOptions recipe={recipe} onRecipe={onRecipe}/>
        {recipe.processing.draw === "matrix" && !recipeHasMatrix(recipe) && <MatrixOptions recipe={recipe} onRecipe={onRecipe}/>}
      </section>
    </Group>
    <Group {...group("pasted")} title={t("После вставки")} summary={summaries.pasted} phase="pasted"
      hint={t("Короткая заметка о том, что текст вставлен. Она исчезает сама")}>
      <section className="ove-sec"><PastedOptions recipe={recipe} onRecipe={onRecipe}/></section>
    </Group>
    <Group {...group("cancel")} title={t("Отмена")} summary={summaries.cancel}
      hint={t("Кнопка есть во всех фазах, кроме заметки о вставке, и её можно переставить")}>
      <section className="ove-sec"><CancelOptions recipe={recipe} onRecipe={onRecipe}/></section>
    </Group>
  </>;
}

/** The cancel button's look, place and visibility. It has no "remove": see `CANCEL_SPOTS`. */
export function CancelOptions({ recipe, onRecipe }: { recipe: Recipe; onRecipe: (recipe: Recipe, message: string) => void }) {
  const draws = cancelDrawNames(), spots = cancelSpotNames(), shows = cancelShowNames();
  const set = (patch: Partial<Recipe["cancel"]>, message: string) => onRecipe({ ...recipe, cancel: { ...recipe.cancel, ...patch } }, message);
  const drawList = cancelDraws(recipe.shell), spotList = CANCEL_SPOTS[recipe.shell], showList = cancelShows(recipe.shell);
  // What this shell cannot have stays in view, dimmed and with the reason, so nobody concludes the option does not exist.
  return <div className="ove-cancel">
    <div className="ove-opts">
      {/* The look speaks for itself: a caption such as «Стоп» read as "stop and keep the text", which this button does not do. */}
      {CANCEL_DRAWS.map((draw) => {
        const fits = drawList.includes(draw);
        return <Hint key={draw} asChild text={fits ? undefined : t("В этом корпусе кнопка лежит поверх деталей: слово не помещается")}>
          <button type="button" className="ove-opt" aria-pressed={recipe.cancel.draw === draw} aria-disabled={!fits}
            aria-label={t("Отмена: «{p0}»", { p0: draws[draw] })} onClick={fits ? () => set({ draw }, t("Отмена: «{p0}»", { p0: draws[draw] })) : undefined}>
            <span className="ove-cancel__pv">{draw === "x" ? <Icon name="x" size={12}/> : draw === "stop" ? <i/> : t("Отмена")}</span>
          </button>
        </Hint>;
      })}
    </div>
    <div className="ove-row">
      <span className="ove-cancel__label">{t("Где")}</span>
      <Segmented value={recipe.cancel.at} disabled={spotList.length < 2} onChange={(at) => set({ at }, t("Отмена: {p0}", { p0: spots[at] }))}
        options={spotList.map((spot) => ({ value: spot, label: spots[spot] }))}/>
      {spotList.length < 2 && <Hint text={t("В этом корпусе у кнопки одно место")}/>}
    </div>
    <div className="ove-row">
      <span className="ove-cancel__label">{t("Видна")}</span>
      <Segmented value={recipe.cancel.show} disabled={showList.length < 2}
        onChange={(show) => set({ show: show as Recipe["cancel"]["show"] }, t("Отмена: {p0}", { p0: shows[show as Recipe["cancel"]["show"]].toLowerCase() }))}
        options={CANCEL_SHOWS.map((show) => ({ value: show, label: shows[show] }))}/>
      {showList.length < 2 && <Hint text={t("В этом корпусе кнопка лежит поверх деталей, поэтому появляется только при наведении")}/>}
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
    <SectionHead title={t("Матрица")} hint={t("Узор во время речи и обработки, плотность точек")}/>
    {recipeHasMatrix(recipe) && <>
      <p className="ove-matrix__title">{t("Когда вы говорите")}</p>
      <div className="ove-mxopts">{MATRIX_SPEECH.map((value) => tile(value, recipe.matrix.speech === value, speech[value][0], speech[value][1],
        () => set({ speech: value }, t("Матрица: «{p0}»", { p0: speech[value][0] })), { ...recipe.matrix, speech: value }, "speech"))}</div>
    </>}
    {recipe.processing.draw === "matrix" && <>
      <p className="ove-matrix__title">{t("Пока текст обрабатывается")}</p>
      <div className="ove-mxopts">{MATRIX_PROCESS.map((value) => tile(value, recipe.matrix.process === value, process[value][0], process[value][1],
        () => set({ process: value }, t("Обработка: «{p0}». Фаза «Обработка» под макетом покажет её в корпусе", { p0: process[value][0] })), { ...recipe.matrix, process: value }, "process"))}</div>
    </>}
    <p className="ove-matrix__title">{t("Плотность")}</p>
    <div className="ove-mxopts ove-mxopts--three">{MATRIX_DENSITIES.map((value) => tile(String(value), recipe.matrix.density === value, `${value} × ${value}`,
      value === 5 ? t("крупно") : value === 7 ? t("средне") : t("плотно"),
      () => set({ density: value }, t("Матрица: плотность {p0} × {p0}", { p0: value })), { ...recipe.matrix, density: value }, "speech"))}</div>
  </div>;
}

function StyleTab({ recipe, preferences, onRecipe, onLook, onLookPreview, onLookCommit }: Props) {
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
      <PaletteTone palette={preferences} onChange={onLookPreview} onCommit={onLookCommit}/>
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
      <SectionHead title={t("Движение")} hint={t("Как корпус переходит между состояниями")}/>
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
