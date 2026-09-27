import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Hint } from "../components/Hint";
import { Icon } from "../components/Icon";
import { Segmented } from "../components/Shell";
import type { ConfigResult } from "../bridge/types";
import { t } from "../i18n";
import { OverlayScene } from "../overlay/OverlayScene";
import { overlayPalette } from "../overlay/overlayPalette";
import {
  DRAWINGS, SHELL_REGIONS, WINDOW_SIZE, compatible, recipeHasMatrix, recipeLayout, regionOf,
  type ElementType, type Recipe, type Shell, type UserTemplate,
} from "../overlay/overlayRecipe";
import { EditorLibrary, PartPreview, type LibraryTab } from "./overlayEditor/EditorLibrary";
import { drawNames, elementNames, kindPlaces, regionNames, shellNames } from "./overlayEditor/labels";
import { currentRecipe, matchingTemplate, newTemplate, preferencesOf, templateLook, useOverlaySaver, type ConfigChange, type OverlayLook } from "./overlayEditor/overlayDraft";
import { addPart, changeShell, drawingKinds, placeInto, removePart, type EditResult } from "./overlayEditor/recipeEdits";
import { frameAt, PHASE_MODES, samplePhrase, simulatedVoice, silentVoice, useTicker, type PhaseMode } from "./overlayEditor/simulatedVoice";
import { NameDialog } from "./overlayEditor/TemplateTiles";
import { SYSTEM_TEMPLATES, systemTemplateNames, type SystemTemplate } from "./overlayEditor/templates";

type Snapshot = { recipe: Recipe; look: OverlayLook };
type Drag = { type: ElementType; draw: string | null; from: string | null; x: number; y: number; moved: boolean; target: string | null; reason: string };
const HISTORY_LIMIT = 60;
const SCREEN = { width: 1920, height: 1080 };

const lookOf = (preferences: ReturnType<typeof preferencesOf>): OverlayLook => ({
  palette: preferences.palette, palette_hue: preferences.palette_hue, palette_chroma: preferences.palette_chroma,
  size: preferences.size, anchor: preferences.anchor, edge_offset: preferences.edge_offset,
});

/** The window's top-left corner on a 1920×1080 screen, as overlay_origin places it natively. */
function windowOrigin(anchor: string, offset: number, width: number, height: number) {
  const [vertical, horizontal] = anchor === "center" ? ["center", "center"] : anchor.split("-");
  const x = horizontal === "left" ? offset : horizontal === "right" ? SCREEN.width - width - offset : (SCREEN.width - width) / 2;
  const y = vertical === "top" ? offset : vertical === "bottom" ? SCREEN.height - height - offset : (SCREEN.height - height) / 2;
  return [Math.max(0, Math.min(SCREEN.width - width, x)), Math.max(0, Math.min(SCREEN.height - height, y))];
}

export function OverlayEditor({ config, onConfigChanged, onClose }: { config: ConfigResult | null; onConfigChanged: ConfigChange; onClose: () => void }) {
  const [preferences, setPreferences] = useState(() => preferencesOf(config));
  const [recipe, setRecipe] = useState(() => currentRecipe(preferencesOf(config)));
  const [history, setHistory] = useState<Snapshot[]>([]);
  const saver = useOverlaySaver(config, onConfigChanged);
  const [tab, setTab] = useState<LibraryTab>("build");
  const [view, setView] = useState<"compose" | "screen">("compose");
  const [phaseMode, setPhaseMode] = useState<PhaseMode>("recording");
  const [phaseSince, setPhaseSince] = useState(() => performance.now());
  const [selected, setSelected] = useState<ElementType | null>(null);
  const [toast, setToast] = useState<{ text: string; warn: boolean } | null>(null);
  const [naming, setNaming] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const [stageWidth, setStageWidth] = useState(640);
  const now = useTicker(true, 250);
  const frame = frameAt(phaseMode, now - phaseSince);

  const names = { elements: elementNames(), draws: drawNames(), regions: regionNames(), shells: shellNames(), places: kindPlaces() };

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 5200);
    return () => window.clearTimeout(timer);
  }, [toast]);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const observer = new ResizeObserver(([entry]) => setStageWidth(entry.contentRect.width));
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  function remember() {
    setHistory((items) => [...items.slice(-HISTORY_LIMIT + 1), { recipe, look: lookOf(preferences) }]);
  }
  function commitRecipe(next: Recipe, message?: string, look: Partial<OverlayLook> = {}, warn = false) {
    remember();
    setRecipe(next);
    if (Object.keys(look).length) setPreferences((current) => ({ ...current, ...look }));
    void saver.saveRecipe(next, look);
    if (message) setToast({ text: message, warn });
  }
  function commitLook(look: Partial<OverlayLook>) {
    remember();
    setPreferences((current) => ({ ...current, ...look }));
    void saver.save(look);
  }
  function undo() {
    const last = history[history.length - 1];
    if (!last) return;
    setHistory((items) => items.slice(0, -1));
    setRecipe(last.recipe);
    setPreferences((current) => ({ ...current, ...last.look }));
    void saver.saveRecipe(last.recipe, last.look);
    setToast({ text: t("Последнее изменение отменено"), warn: false });
  }
  function saveTemplates(templates: UserTemplate[], saved?: UserTemplate) {
    setPreferences((current) => ({ ...current, templates }));
    void saver.saveTemplates(templates);
    if (saved) setToast({ text: t("Шаблон «{p0}» сохранён", { p0: saved.name }), warn: false });
  }
  function reasonFor(type: ElementType, draw: string, region: string) {
    if (type === "draft") return t("Черновику нужна строка для текста");
    return t("«{p0}» не помещается в «{p1}». Подходит: {p2}", {
      p0: names.draws[type][draw], p1: names.regions[region],
      p2: drawingKinds(type, draw).map((kind) => names.places[kind]).join(", "),
    });
  }
  function apply(result: EditResult, message: string) {
    if (result.ok) { commitRecipe(result.recipe, message); return true; }
    setToast({
      text: result.reason === "no-room"
        ? t("Свободного места для «{p0}» нет. Перетащите деталь на занятую область, чтобы заменить.", { p0: names.draws[result.type][result.draw] })
        : result.region ? reasonFor(result.type, result.draw, result.region)
        : t("В корпус «{p0}» не помещается. Подходит: {p1}", { p0: names.shells[recipe.shell], p1: drawingKinds(result.type, result.draw).map((kind) => names.places[kind]).join(", ") }),
      warn: true,
    });
    return false;
  }
  function onShell(shell: Shell) {
    if (shell === recipe.shell) return;
    const result = changeShell(recipe, shell);
    let message = t("Корпус: {p0}.", { p0: names.shells[shell] });
    if (result.switched?.length) message += " " + t("Рисунок сменился: {p0}.", { p0: result.switched.map((type) => `${names.elements[type].toLowerCase()} → «${names.draws[type][result.recipe.draw[type]]}»`).join(", ") });
    if (result.left?.length) message += " " + t("Не поместились: {p0}.", { p0: result.left.map((type) => names.elements[type].toLowerCase()).join(", ") });
    setSelected(null);
    commitRecipe(result.recipe, message, {}, !!result.left?.length);
  }
  function setPhase(mode: PhaseMode) { setPhaseMode(mode); setPhaseSince(performance.now()); }

  // Keyboard: undo and closing the popover.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = (event.target as HTMLElement | null)?.closest?.("input, textarea");
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z" && !typing) { event.preventDefault(); undo(); }
      if (event.key === "Escape" && selected) setSelected(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Drag and drop between the library and the regions of the scene.
  const dragRef = useRef<Drag | null>(null);
  function startDrag(event: ReactPointerEvent, type: ElementType, draw: string | null, from: string | null) {
    if (event.button !== 0) return;
    const start = { x: event.clientX, y: event.clientY };
    dragRef.current = { type, draw, from, x: start.x, y: start.y, moved: false, target: null, reason: "" };
    const targets = () => Array.from(stageRef.current?.querySelectorAll<HTMLElement>("[data-region]") ?? []);
    const fits = (region: string) => {
      const kinds = SHELL_REGIONS[recipe.shell][region];
      return !!kinds && (draw ? compatible(type, draw, kinds) : Object.keys(DRAWINGS[type]).some((option) => compatible(type, option, kinds)));
    };
    const move = (moveEvent: PointerEvent) => {
      const current = dragRef.current;
      if (!current) return;
      if (!current.moved && Math.hypot(moveEvent.clientX - start.x, moveEvent.clientY - start.y) < 5) return;
      if (!current.moved && !["recording", "streaming", "limit"].includes(phaseMode)) {
        setPhase("recording");
        setToast({ text: t("Показываю запись: в ней видны все области"), warn: false });
      }
      setSelected(null);
      const hit = document.elementFromPoint(moveEvent.clientX, moveEvent.clientY)?.closest<HTMLElement>("[data-region]");
      const target = hit && stageRef.current?.contains(hit) ? hit.dataset.region ?? null : null;
      for (const element of targets()) {
        const region = element.dataset.region!;
        element.dataset.drop = region === target ? (fits(region) ? "hot" : "hot-no") : fits(region) ? "ok" : "no";
      }
      const reason = target && !fits(target) ? reasonFor(type, draw ?? recipe.draw[type], target) : "";
      dragRef.current = { ...current, x: moveEvent.clientX, y: moveEvent.clientY, moved: true, target, reason };
      setDrag(dragRef.current);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      for (const element of targets()) delete element.dataset.drop;
      const current = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      if (!current?.moved) {
        if (from) setSelected(type);
        return;
      }
      if (!current.target) return;
      const result = placeInto(recipe, type, draw, current.target);
      apply(result, t("{p0} → «{p1}»", { p0: names.elements[type], p1: names.regions[current.target] }));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  const match = matchingTemplate(recipe, preferences);
  const subtitle = match?.kind === "mine" ? t("мой шаблон «{p0}»", { p0: match.template.name })
    : match ? t("шаблон «{p0}»", { p0: systemTemplateNames()[match.key] }) : t("своя сборка");
  const draftOpen = frame.phase === "recording" && frame.streaming && recipe.slots.below === "draft";
  const layout = recipeLayout(recipe, draftOpen, frame.phase === "error");
  const [windowWidth, windowHeight] = WINDOW_SIZE[layout][preferences.size];
  const composeScale = Math.max(0.5, Math.min(1.6, (stageWidth - 48) / windowWidth, 220 / windowHeight));
  const screenScale = stageWidth / SCREEN.width;
  const [originX, originY] = windowOrigin(preferences.anchor, preferences.edge_offset, windowWidth, windowHeight);
  const ghosts: Record<string, string> = {};
  if (SHELL_REGIONS[recipe.shell].below) ghosts.below = recipe.slots.below ? t("Черновик: раскроет карточку с первым словом") : t("строка текста · сюда черновик");
  if (SHELL_REGIONS[recipe.shell].edge) ghosts.edge = recipe.slots.edge ? t("Край: {p0}", { p0: names.draws.level[recipe.draw.level].toLowerCase() }) : t("край · сюда луч");
  const language = (config?.language || "ru").toUpperCase();
  const statusText = frame.phase === "processing" ? t("Обрабатываю") : frame.phase === "pasted"
    ? t("{p0} символов вставлено", { p0: samplePhrase().length }) : frame.phase === "error" ? t("Не удалось открыть микрофон") : "";

  const scene = <OverlayScene recipe={recipe} size={preferences.size} phase={frame.phase} shown={frame.shown} streaming={frame.streaming}
    draft={frame.draft} draftPlaceholder={t("Говорите — текст появится здесь")} timer={frame.timer} limited={frame.limited}
    status={statusText} mode={{ full: config?.model ? `${language} · ${config.model}` : language, short: language }}
    source={frame.phase === "recording" ? simulatedVoice : silentVoice} close={{ label: t("Отменить запись") }}
    interactive={view === "compose" ? { labels: names.regions, ghosts, selected } : undefined}/>;

  return <div className="page ove" data-testid="overlay-editor">
    <div className="ove-head">
      <button type="button" className="btn btn--ghost" onClick={onClose}><Icon name="chev-left" size={14}/>{t("Настройки")}</button>
      <div className="ove-head__title"><h1 className="page-title">{t("Оверлей")}</h1><span>{subtitle}</span></div>
      <span className="ove-head__grow"/>
      <Hint text={match ? t("Текущая настройка уже есть среди шаблонов") : t("Сохранить текущий оверлей как свой шаблон")}>
        <span><button type="button" className="btn" disabled={match !== null} onClick={() => setNaming(true)}>
          <Icon name="plus" size={14}/>{t("Сохранить как шаблон")}
        </button></span>
      </Hint>
      <button type="button" className="btn" disabled={!history.length} onClick={undo}>{t("Отменить")}</button>
      <button type="button" className="btn" onClick={() => commitRecipe(structuredClone(SYSTEM_TEMPLATES.pill), t("Сброшено к пилюле. Место на экране не изменилось. «Отменить» вернёт как было"))}>{t("Сбросить")}</button>
      <button type="button" className="btn btn--primary" onClick={onClose}>{t("Готово")}</button>
    </div>
    {saver.error && <p className="overlay-settings-error" role="alert">{saver.error}</p>}
    <div className="ove-grid">
      <EditorLibrary tab={tab} onTab={setTab} recipe={recipe} preferences={preferences}
        onRecipe={(next, message) => commitRecipe(next, message)} onShell={onShell}
        onAddPart={(type, draw) => {
          const result = addPart(recipe, type, draw);
          const before = regionOf(recipe, type), after = result.ok ? regionOf(result.recipe, type) : null;
          apply(result, before && before === after
            ? t("{p0}: «{p1}»", { p0: names.elements[type], p1: names.draws[type][draw] })
            : t("{p0} → «{p1}»", { p0: names.elements[type], p1: after ? names.regions[after] : "" }));
        }}
        onPartPointerDown={(event, type, draw) => startDrag(event, type, draw, null)}
        onLook={commitLook}
        onApplySystem={(key: SystemTemplate) => { setSelected(null); commitRecipe(structuredClone(SYSTEM_TEMPLATES[key]), t("Шаблон «{p0}». Цвет, размер и место сохранены", { p0: systemTemplateNames()[key] })); }}
        onApplyMine={(template) => { setSelected(null); commitRecipe(structuredClone(template.recipe), t("Мой шаблон «{p0}». Место на экране не изменилось", { p0: template.name }), templateLook(template)); }}
        onTemplates={saveTemplates}/>
      <div className="card ove-stagecard">
        <div className="ove-bar">
          <Segmented value={view} onChange={(value) => { setView(value as "compose" | "screen"); setSelected(null); }}
            options={[{ value: "compose", label: t("Состав") }, { value: "screen", label: t("На экране") }]}/>
          <span className="ove-bar__note">{t("окно {p0}×{p1}", { p0: windowWidth, p1: windowHeight })}</span>
        </div>
        <div className="ove-stage" ref={stageRef} data-view={view} style={overlayPalette(preferences)}
          onPointerDown={(event) => {
            const region = (event.target as HTMLElement).closest<HTMLElement>("[data-region]");
            const type = region?.dataset.filled as ElementType | undefined;
            if (region && type && view === "compose") startDrag(event, type, null, region.dataset.region ?? null);
            else if (!(event.target as HTMLElement).closest(".ove-pop")) setSelected(null);
          }}
          onKeyDown={(event) => {
            const region = (event.target as HTMLElement).closest<HTMLElement>("[data-region]");
            const type = region?.dataset.filled as ElementType | undefined;
            if (!type) return;
            if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setSelected(type); }
            if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); commitRecipe(removePart(recipe, type), t("{p0} убран", { p0: names.elements[type] })); }
          }}>
          <div className="ove-stage__view">
          {view === "compose"
            ? <div className="ove-compose">
              <div className="ove-window" style={{ width: windowWidth, height: windowHeight, transform: `scale(${composeScale.toFixed(3)})` }}>{scene}</div>
            </div>
            : <div className="ove-screen" style={{ width: SCREEN.width, height: SCREEN.height, transform: `scale(${screenScale.toFixed(4)})` }}>
              <div className="ove-screen__doc"><i/><i/><i/><i/><i/><i/></div>
              <div className="ove-screen__window" style={{ left: originX, top: originY, width: windowWidth, height: windowHeight }}>{scene}</div>
            </div>}
          </div>
          {selected && view === "compose" && <Popover recipe={recipe} type={selected} stageRef={stageRef} names={names}
            onPick={(draw) => { const region = regionOf(recipe, selected); if (region) apply(placeInto(recipe, selected, draw, region), t("{p0}: «{p1}»", { p0: names.elements[selected], p1: names.draws[selected][draw] })); }}
            onRemove={() => { commitRecipe(removePart(recipe, selected), t("{p0} убран", { p0: names.elements[selected] })); setSelected(null); }}
            onMatrix={() => { setSelected(null); setTab("build"); requestAnimationFrame(() => document.getElementById("overlay-matrix-options")?.scrollIntoView({ block: "nearest", behavior: "smooth" })); }}
            onClose={() => setSelected(null)}/>}
        </div>
        <div className="ove-bar">
          <Segmented value={phaseMode} onChange={(value) => setPhase(value as PhaseMode)} options={PHASE_MODES.map((mode) => ({ value: mode, label: {
            scenario: t("Сценарий"), recording: t("Запись"), streaming: t("Стриминг"), processing: t("Обработка"),
            pasted: t("Вставка"), error: t("Ошибка"), limit: t("Лимит"),
          }[mode] }))}/>
        </div>
        <p className={`ove-toast${toast?.warn ? " ove-toast--warn" : ""}`} role="status" aria-live="polite">{toast?.text ?? (view === "compose" ? t("Перетащите деталь из библиотеки в область или нажмите её. Нажмите элемент в макете, чтобы сменить рисунок.") : "")}</p>
        <AfterRecording recipe={recipe} preferences={preferences} names={names}/>
      </div>
    </div>
    {drag?.moved && createPortal(<div className="ove-ghost" style={{ left: drag.x + 14, top: drag.y + 14 }}>
      <span className="ove-ghost__pv" style={overlayPalette(preferences)}><PartPreview type={drag.type} draw={drag.draw ?? recipe.draw[drag.type]} recipe={recipe}/></span>
      <b>{names.elements[drag.type]}</b>
      {drag.reason && <em>{drag.reason}</em>}
    </div>, document.body)}
    {naming && <NameDialog title={t("Сохранить как шаблон")} action={t("Сохранить")}
      initial={t("Мой оверлей {p0}", { p0: preferences.templates.length + 1 })} onCancel={() => setNaming(false)}
      onSubmit={(name) => { setNaming(false); const created = newTemplate(name, recipe, preferences); saveTemplates([...preferences.templates, created], created); }}/>}
  </div>;
}

type Names = { elements: ReturnType<typeof elementNames>; draws: ReturnType<typeof drawNames>; regions: Record<string, string>; shells: ReturnType<typeof shellNames>; places: ReturnType<typeof kindPlaces> };

/** Visual alternatives for the element picked in the scene. */
function Popover({ recipe, type, stageRef, names, onPick, onRemove, onMatrix, onClose }: {
  recipe: Recipe; type: ElementType; stageRef: RefObject<HTMLDivElement | null>; names: Names;
  onPick: (draw: string) => void; onRemove: () => void; onMatrix: () => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<CSSProperties>({ visibility: "hidden" });
  const region = regionOf(recipe, type);
  useLayoutEffect(() => {
    const stage = stageRef.current, pop = ref.current;
    const target = region ? stage?.querySelector<HTMLElement>(`[data-region="${region}"]`) : null;
    if (!stage || !pop || !target) return;
    const bounds = stage.getBoundingClientRect(), anchor = target.getBoundingClientRect();
    // Below the element; above it only when the window has no room underneath.
    let top = anchor.bottom - bounds.top + 8;
    if (anchor.bottom + 8 + pop.offsetHeight > window.innerHeight && anchor.top - pop.offsetHeight - 8 > 0) top = anchor.top - bounds.top - pop.offsetHeight - 8;
    const left = Math.max(4, Math.min(bounds.width - pop.offsetWidth - 4, anchor.left - bounds.left + anchor.width / 2 - pop.offsetWidth / 2));
    setPosition({ top, left });
  }, [region, recipe, stageRef]);
  if (!region) return null;
  const kinds = SHELL_REGIONS[recipe.shell][region];
  const draws = Object.keys(DRAWINGS[type]);
  const others = draws.filter((draw) => !compatible(type, draw, kinds)).map((draw) => names.draws[type][draw]);
  return <div className="ove-pop" ref={ref} style={position} role="dialog" aria-label={t("{p0}: варианты", { p0: names.elements[type] })}>
    <div className="ove-pop__head">
      <span><b>{names.elements[type]}</b> · {names.regions[region]}</span>
      <span>
        <button type="button" className="btn btn--ghost" onClick={onRemove}>{t("Убрать")}</button>
        <button type="button" className="btn btn--ghost" aria-label={t("Закрыть")} onClick={onClose}><Icon name="x" size={13}/></button>
      </span>
    </div>
    <div className="ove-parts">
      {draws.filter((draw) => compatible(type, draw, kinds)).map((draw) => <button key={draw} type="button"
        className={`ove-part${type === "draft" || draw === "scope" || draw === "beam" ? " ove-part--wide" : ""}`}
        aria-pressed={recipe.draw[type] === draw} onClick={() => onPick(draw)}>
        <span className="ove-part__pv"><PartPreview type={type} draw={draw} recipe={recipe}/></span>
        <span className="ove-part__meta"><b>{names.draws[type][draw]}</b></span>
      </button>)}
    </div>
    {others.length > 0 && <p className="ove-pop__note">{t("Сюда не помещаются: {p0}.", { p0: others.join(", ") })}</p>}
    {type === "level" && recipe.draw.level === "matrix" && <button type="button" className="btn" onClick={onMatrix}>{t("Узор и плотность матрицы")} <Icon name="arrow-right" size={13}/></button>}
  </div>;
}

/** What the overlay does once the recording ends: the states the recipe does not choose. */
function AfterRecording({ recipe, preferences, names }: { recipe: Recipe; preferences: ReturnType<typeof preferencesOf>; names: Names }) {
  const shell = recipe.shell, round = shell === "bead" || shell === "stack", draft = regionOf(recipe, "draft");
  const pill = WINDOW_SIZE.pill[preferences.size].join("×"), stream = WINDOW_SIZE.streaming[preferences.size].join("×");
  const rows: Array<[string, string]> = [
    [t("Обработка"), recipeHasMatrix(recipe) ? t("матрица показывает узор обработки") : round ? t("в корпусе бежит сегмент по кругу")
      : shell === "card" ? t("статус встаёт в тело, остальное уходит") : shell === "caps" ? t("статус в чипе, по словам проходит блик")
      : shell === "island" ? t("строка сжимается под статус") : t("статус занимает всю строку")],
    [t("Вставка"), round ? t("галочка в корпусе") : t("галочка и число вставленных символов")],
    [t("Ошибка"), round ? t("корпус раскрывается в пилюлю {p0}", { p0: pill }) : t("текст ошибки на месте статуса")],
    [t("Текст модели"), !draft ? t("не показывается: черновика нет") : draft === "below" ? t("окно переходит в {p0}, пока идёт стриминг", { p0: stream }) : t("в области «{p0}»", { p0: names.regions[draft] })],
    [t("Отсчёт лимита"), regionOf(recipe, "timer") ? t("таймер считает назад цветом предупреждения") : t("таймера нет, отсчёт появится над корпусом")],
    [t("Отмена"), t("по наведению или с клавиатуры, всегда в корпусе")],
  ];
  const warnings: string[] = [];
  if (!regionOf(recipe, "level") && !regionOf(recipe, "timer") && !regionOf(recipe, "rec")) warnings.push(t("Во время записи оверлей пуст: добавьте уровень, таймер или индикатор."));
  if (recipe.style.fill === "none" && (regionOf(recipe, "timer") || regionOf(recipe, "mode") || (draft && draft !== "lines"))) warnings.push(t("Без заливки цифры и текст теряются на светлом документе."));
  if (shell === "island" && !preferences.anchor.startsWith("top")) warnings.push(t("Остров задуман у верхнего края."));
  return <details className="ove-after" open>
    <summary>{t("После записи оверлей разложится так")}</summary>
    <dl>{rows.map(([term, text]) => <div key={term}><dt>{term}</dt><dd>{text}</dd></div>)}</dl>
    {warnings.length > 0 && <ul className="ove-warns">{warnings.map((text) => <li key={text}>{text}</li>)}</ul>}
  </details>;
}

