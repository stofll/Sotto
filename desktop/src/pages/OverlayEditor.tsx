import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Hint } from "../components/Hint";
import { Icon } from "../components/Icon";
import { Segmented } from "../components/Shell";
import type { ConfigResult } from "../bridge/types";
import { localeTag, t } from "../i18n";
import { OverlayScene } from "../overlay/OverlayScene";
import { overlayPalette } from "../overlay/overlayPalette";
import {
  DRAWINGS, MAX_TEMPLATES, COMPACT_SHELLS, SHELL_REGIONS, WINDOW_SIZE, compatible, recipeLayout, regionOf,
  type ElementType, type Recipe, type RecipeStyle, type Shell, type UserTemplate,
} from "../overlay/overlayRecipe";
import { CancelOptions, EditorLibrary, isWidePart, PartPreview, type LibraryTab } from "./overlayEditor/EditorLibrary";
import { cancelShowNames, cancelSpotNames, drawNames, elementNames, kindPlaces, processNames, regionNames, shellNames, styleNames } from "./overlayEditor/labels";
import { currentRecipe, matchingTemplate, newTemplate, preferencesOf, templateLook, updatedTemplate, useOverlaySaver, type ConfigChange, type OverlayLook } from "./overlayEditor/overlayDraft";
import { addPart, changeShell, drawingKinds, freePartsFor, partsFor, placeInto, removePart, restoreShell, type EditResult } from "./overlayEditor/recipeEdits";
import { frameAt, PHASE_MODES, samplePhrase, simulatedVoice, silentVoice, useTicker, type PhaseMode } from "./overlayEditor/simulatedVoice";
import { RadiusHandle } from "./overlayEditor/RadiusHandle";
import { NameDialog } from "./overlayEditor/TemplateTiles";
import { SYSTEM_TEMPLATES, systemTemplateNames, type SystemTemplate } from "./overlayEditor/templates";

type Snapshot = { recipe: Recipe; look: OverlayLook };
/** What the stage popover is about: an element, the cancel button, or an empty region to fill. */
type Selection = ElementType | "cancel" | { region: string } | null;
/** `remove`: a part dragged out of the shell and released there leaves it. */
type Drag = { type: ElementType; draw: string | null; from: string | null; x: number; y: number; moved: boolean; target: string | null; reason: string; remove: boolean };
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

export function OverlayEditor({ config, onConfigChanged, onClose, template }: {
  config: ConfigResult | null; onConfigChanged: ConfigChange; onClose: () => void;
  /** The id of a user template to edit; the settings card has already applied it. */
  template?: string;
}) {
  const [preferences, setPreferences] = useState(() => preferencesOf(config));
  const [recipe, setRecipe] = useState(() => currentRecipe(preferencesOf(config)));
  const [history, setHistory] = useState<Snapshot[]>([]);
  const [editingId, setEditingId] = useState(template ?? null);
  const saver = useOverlaySaver(config, onConfigChanged);
  const [tab, setTab] = useState<LibraryTab>("build");
  const [view, setView] = useState<"compose" | "screen">("compose");
  const [phaseMode, setPhaseMode] = useState<PhaseMode>("recording");
  const [phaseSince, setPhaseSince] = useState(() => performance.now());
  const [selected, setSelected] = useState<Selection>(null);
  const [toast, setToast] = useState<{ text: string; warn: boolean } | null>(null);
  const [naming, setNaming] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  // The cancel button sits on top of the level in a small round shell, so the preview hides it there until asked.
  const [cancelShown, setCancelShown] = useState<boolean | null>(null);
  // The corner handle previews a radius while it is dragged and commits it on release.
  const [radiusDraft, setRadiusDraft] = useState<RecipeStyle["radius"] | null>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const [stageBox, setStageBox] = useState({ width: 640, height: 360 });
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
    const observer = new ResizeObserver(([entry]) => setStageBox({ width: entry.contentRect.width, height: entry.contentRect.height }));
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  function remember() {
    setHistory((items) => [...items.slice(-HISTORY_LIMIT + 1), { recipe, look: lookOf(preferences) }]);
  }
  const editVersion = useRef(0);
  function saveView(next: Recipe, look: OverlayLook) {
    const version = ++editVersion.current;
    // Save the whole visible draft: a later edit must also carry changes whose
    // earlier save failed. Only the latest failure may roll the preview back.
    void saver.saveRecipe(next, look).then((result) => {
      if (result || version !== editVersion.current) return;
      const restored = preferencesOf(saver.saved.current);
      setPreferences(restored);
      setRecipe(currentRecipe(restored));
      setHistory([]);
      setSelected(null);
      setToast(null);
    });
  }
  function commitRecipe(edited: Recipe, message?: string, look: Partial<OverlayLook> = {}, warn = false) {
    const next = restoreShell(recipe, edited);
    remember();
    setRecipe(next);
    if (Object.keys(look).length) setPreferences((current) => ({ ...current, ...look }));
    saveView(next, { ...lookOf(preferences), ...look });
    if (message) setToast({ text: message, warn });
  }
  function commitLook(look: Partial<OverlayLook>) {
    remember();
    setPreferences((current) => ({ ...current, ...look }));
    saveView(recipe, { ...lookOf(preferences), ...look });
  }
  // A slider drag is one change: history keeps the look from before the drag, and it is saved on release.
  const lookDrag = useRef(false);
  function previewLook(look: Partial<OverlayLook>) {
    editVersion.current++;
    if (!lookDrag.current) { remember(); lookDrag.current = true; }
    setPreferences((current) => ({ ...current, ...look }));
  }
  function commitPreviewedLook(look: Partial<OverlayLook>) {
    if (!lookDrag.current) return;
    lookDrag.current = false;
    saveView(recipe, { ...lookOf(preferences), ...look });
  }
  function undo() {
    const last = history[history.length - 1];
    if (!last) return;
    setHistory((items) => items.slice(0, -1));
    setRecipe(last.recipe);
    setPreferences((current) => ({ ...current, ...last.look }));
    saveView(last.recipe, last.look);
    setToast({ text: t("Последнее изменение отменено"), warn: false });
  }
  async function saveTemplates(templates: UserTemplate[], saved?: UserTemplate) {
    if (templates.length > MAX_TEMPLATES) return;
    const result = await saver.saveTemplates(templates);
    if (!result) return;
    setPreferences((current) => ({ ...current, templates }));
    if (saved) setToast({ text: t("Шаблон «{p0}» сохранён", { p0: saved.name }), warn: false });
  }
  function reasonFor(type: ElementType, draw: string, region: string) {
    if (type === "draft") return t("Черновику нужна строка для текста");
    return t("«{p0}» не помещается в «{p1}». Подходит: {p2}", {
      p0: names.draws[type][draw], p1: names.regions[region],
      p2: drawingKinds(type, draw).map((kind) => names.places[kind]).join(", "),
    });
  }
  /** Says which elements an edit had to redraw to fit their new place. */
  function switchedNote(result: EditResult & { ok: true }) {
    return result.switched?.length
      ? t("Рисунок сменился: {p0}.", { p0: result.switched.map((type) => `${names.elements[type].toLowerCase()} → «${names.draws[type][result.recipe.draw[type]]}»`).join(", ") })
      : "";
  }
  function apply(result: EditResult, message: string) {
    if (result.ok) { commitRecipe(result.recipe, [message, switchedNote(result)].filter(Boolean).join(". ")); return true; }
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
    if (result.switched?.length) message += " " + switchedNote(result);
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
    dragRef.current = { type, draw, from, x: start.x, y: start.y, moved: false, target: null, reason: "", remove: false };
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
      const under = document.elementFromPoint(moveEvent.clientX, moveEvent.clientY);
      const hit = under?.closest<HTMLElement>("[data-region]");
      const target = hit && stageRef.current?.contains(hit) ? hit.dataset.region ?? null : null;
      // Off the shell and off every region: a part taken from the shell would leave it.
      const remove = from !== null && !target && !under?.closest(".ovs-skin, .ovs-rg-lines");
      for (const element of targets()) {
        const region = element.dataset.region!;
        element.dataset.drop = region === target ? (fits(region) ? "hot" : "hot-no") : fits(region) ? "ok" : "no";
      }
      const reason = target && !fits(target) ? reasonFor(type, draw ?? recipe.draw[type], target) : "";
      if (from) {
        const origin = stageRef.current?.querySelector<HTMLElement>(`[data-region="${from}"]`);
        if (origin) origin.dataset.leaving = remove ? "1" : "";
      }
      dragRef.current = { ...current, x: moveEvent.clientX, y: moveEvent.clientY, moved: true, target, reason, remove };
      setDrag(dragRef.current);
    };
    const up = (upEvent: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      for (const element of targets()) { delete element.dataset.drop; delete element.dataset.leaving; }
      const current = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      // A cancelled gesture (the system took the pointer) changes nothing.
      if (upEvent.type === "pointercancel") return;
      if (!current?.moved) {
        if (from) setSelected(type);
        return;
      }
      if (current.remove) {
        commitRecipe(removePart(recipe, type), t("{p0} убран", { p0: names.elements[type] }));
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

  const showCancel = cancelShown ?? !COMPACT_SHELLS.includes(recipe.shell);
  const match = matchingTemplate(recipe, preferences);
  const editing = preferences.templates.find((item) => item.id === editingId) ?? null;
  const editingSaved = editing !== null && match?.kind === "mine" && match.template.id === editing.id;
  const subtitle = editing ? t("правка шаблона «{p0}»", { p0: editing.name })
    : match?.kind === "mine" ? t("мой шаблон «{p0}»", { p0: match.template.name })
    : match ? t("шаблон «{p0}»", { p0: systemTemplateNames()[match.key] }) : null;
  const draftOpen = frame.phase === "recording" && frame.streaming && recipe.slots.below === "draft";
  const layout = recipeLayout(recipe, draftOpen, frame.phase === "error");
  const [windowWidth, windowHeight] = WINDOW_SIZE[layout][preferences.size];
  const composeScale = Math.max(0.5, Math.min(1.6, (stageBox.width - 48) / windowWidth, Math.min(220, stageBox.height - 40) / windowHeight));
  // A narrow window caps the stage's height, so the screen is letterboxed rather than cropped.
  const screenScale = Math.min(stageBox.width / SCREEN.width, stageBox.height / SCREEN.height);
  const screenLeft = (stageBox.width - SCREEN.width * screenScale) / 2;
  const [originX, originY] = windowOrigin(preferences.anchor, preferences.edge_offset, windowWidth, windowHeight);
  const ghosts: Record<string, string> = {};
  if (SHELL_REGIONS[recipe.shell].below) ghosts.below = recipe.slots.below ? t("Черновик: раскроет карточку с первым словом") : t("строка текста · сюда черновик");
  if (SHELL_REGIONS[recipe.shell].edge) ghosts.edge = recipe.slots.edge ? t("Край: {p0}", { p0: names.draws.level[recipe.draw.level].toLowerCase() }) : t("край · сюда луч");
  const language = (config?.language || "ru").toUpperCase();
  const statusText = frame.phase === "processing" ? t("Обрабатываю") : frame.phase === "pasted"
    ? t("{p0} символов вставлено", { p0: samplePhrase().length }) : frame.phase === "error" ? t("Не удалось открыть микрофон") : "";

  const shownRecipe = radiusDraft ? { ...recipe, style: { ...recipe.style, radius: radiusDraft } } : recipe;
  const [radiusTitle, radiusNames] = styleNames().radius;
  const scene = <OverlayScene recipe={shownRecipe} size={preferences.size} phase={frame.phase} shown={frame.shown} streaming={frame.streaming}
    draft={frame.draft} draftPlaceholder={t("Говорите — текст появится здесь")} timer={frame.timer} limited={frame.limited}
    status={statusText} mode={{ full: config?.model ? `${language} · ${config.model}` : language, short: language }}
    source={frame.phase === "recording" ? simulatedVoice : silentVoice} close={{ label: t("Отменить запись"), text: t("Отмена") }}
    interactive={view === "compose" ? { labels: names.regions, ghosts, selected: typeof selected === "string" ? selected : null,
      selectedRegion: typeof selected === "object" ? selected?.region : null, cancelLabel: t("Кнопка отмены: где она и как выглядит"), hideCancel: !showCancel } : undefined}/>;

  return <div className="page ove" data-testid="overlay-editor">
    <div className="ove-head">
      <button type="button" className="btn btn--ghost" onClick={onClose}><Icon name="chev-left" size={14}/>{t("Настройки")}</button>
      <div className="ove-head__title"><h1 className="page-title">{t("Оверлей")}</h1>{subtitle && <span>{subtitle}</span>}</div>
      <span className="ove-head__grow"/>
      {editing && <button type="button" className="btn" disabled={editingSaved}
        onClick={() => { const fresh = updatedTemplate(editing, recipe, preferences); saveTemplates(preferences.templates.map((item) => item.id === editing.id ? fresh : item), fresh); }}>
        <Icon name="check" size={14}/>{editingSaved ? t("Шаблон сохранён") : t("Сохранить в «{p0}»", { p0: editing.name })}
      </button>}
      <Hint text={match ? t("Текущая настройка уже есть среди шаблонов") : preferences.templates.length >= MAX_TEMPLATES ? t("Удалите один из своих шаблонов, чтобы сохранить новый") : t("Сохранить текущий оверлей как свой шаблон")}>
        <span><button type="button" className="btn" disabled={match !== null || preferences.templates.length >= MAX_TEMPLATES} onClick={() => setNaming(true)}>
          <Icon name="plus" size={14}/>{t("Сохранить как шаблон")}
        </button></span>
      </Hint>
      <button type="button" className="btn" disabled={!history.length} onClick={undo}>{t("Отменить")}</button>
      <button type="button" className="btn" onClick={() => commitRecipe(structuredClone(SYSTEM_TEMPLATES.pill), t("Сброшено к пилюле. Место на экране не изменилось. «Отменить» вернёт как было"))}>{t("Сбросить")}</button>
      <button type="button" className="btn btn--primary" onClick={onClose}>{t("Готово")}</button>
    </div>
    {saver.error && <p className="overlay-settings-error" role="alert">{saver.error}</p>}
    <div className="ove-grid">
      <EditorLibrary tab={tab} onTab={setTab} recipe={recipe} preferences={preferences} phase={phaseMode}
        onPhase={(mode) => {
          // Any take on a recording (streaming, the limit) already shows the recording parts.
          if (mode === phaseMode || (mode === "recording" && ["streaming", "limit"].includes(phaseMode))) return;
          setPhase(mode);
        }}
        onRecipe={(next, message) => commitRecipe(next, message)} onShell={onShell}
        onAddPart={(type, draw) => {
          const result = addPart(recipe, type, draw);
          const before = regionOf(recipe, type), after = result.ok ? regionOf(result.recipe, type) : null;
          apply(result, before && before === after
            ? t("{p0}: «{p1}»", { p0: names.elements[type], p1: names.draws[type][draw] })
            : t("{p0} → «{p1}»", { p0: names.elements[type], p1: after ? names.regions[after] : "" }));
        }}
        onPartPointerDown={(event, type, draw) => startDrag(event, type, draw, null)}
        onLook={commitLook} onLookPreview={previewLook} onLookCommit={commitPreviewedLook}
        onApplySystem={(key: SystemTemplate) => { setSelected(null); commitRecipe(structuredClone(SYSTEM_TEMPLATES[key]), t("Шаблон «{p0}». Цвет, размер и место сохранены", { p0: systemTemplateNames()[key] })); }}
        onEditMine={(template) => { setSelected(null); setEditingId(template.id); commitRecipe(structuredClone(template.recipe), t("Правка шаблона «{p0}»: изменения сохраняются кнопкой вверху", { p0: template.name }), templateLook(template)); }}
        onApplyMine={(template) => { setSelected(null); commitRecipe(structuredClone(template.recipe), t("Мой шаблон «{p0}». Место на экране не изменилось", { p0: template.name }), templateLook(template)); }}
        onTemplates={saveTemplates}/>
      <div className="card ove-stagecard">
        <div className="ove-bar">
          <Segmented value={view} onChange={(value) => { setView(value as "compose" | "screen"); setSelected(null); }}
            options={[{ value: "compose", label: t("Состав") }, { value: "screen", label: t("На экране") }]}/>
          {view === "compose" && <Hint asChild text={t("Чтобы кнопка не закрывала детали. В оверлее она останется")}>
            <label className="checkbox-row ove-bar__check">
              <input className="checkbox" type="checkbox" checked={!showCancel}
                onChange={(event) => { setCancelShown(!event.target.checked); if (event.target.checked && selected === "cancel") setSelected(null); }}/>
              {t("Скрыть кнопку отмены на превью")}
            </label>
          </Hint>}
          <span className="ove-bar__grow"/>
          <span className="ove-bar__note">{[
            recipe.style.fill === "none" && recipe.style.stroke === "none" ? t("корпус прозрачный: пунктир виден только здесь") : null,
            t("окно {p0}×{p1}", { p0: windowWidth, p1: windowHeight }),
          ].filter(Boolean).join(" · ")}</span>
        </div>
        <div className="ove-stage" ref={stageRef} data-view={view} style={overlayPalette(preferences)}
          onPointerDown={(event) => {
            if (view === "compose" && (event.target as HTMLElement).closest("[data-cancel]")) { setSelected("cancel"); return; }
            const region = (event.target as HTMLElement).closest<HTMLElement>("[data-region]");
            const type = region?.dataset.filled as ElementType | undefined;
            if (region && type && view === "compose") startDrag(event, type, null, region.dataset.region ?? null);
            else if (region?.dataset.region && view === "compose") setSelected({ region: region.dataset.region });
            else if (!(event.target as HTMLElement).closest(".ove-pop")) setSelected(null);
          }}
          onKeyDown={(event) => {
            if ((event.target as HTMLElement).closest("[data-cancel]")) {
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setSelected("cancel"); }
              if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); setToast({ text: cancelKept(), warn: true }); }
              return;
            }
            const region = (event.target as HTMLElement).closest<HTMLElement>("[data-region]");
            const type = region?.dataset.filled as ElementType | undefined;
            if (!type) {
              if (region?.dataset.region && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); setSelected({ region: region.dataset.region }); }
              return;
            }
            if (event.key === "Enter" || event.key === " ") { event.preventDefault(); setSelected(type); }
            if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); commitRecipe(removePart(recipe, type), t("{p0} убран", { p0: names.elements[type] })); }
          }}>
          <div className="ove-stage__view">
          {view === "compose"
            ? <div className="ove-compose">
              <div className="ove-window" style={{ width: windowWidth, height: windowHeight, transform: `scale(${composeScale.toFixed(3)})` }}>{scene}</div>
            </div>
            : <div className="ove-screen" style={{ width: SCREEN.width, height: SCREEN.height, left: screenLeft, transform: `scale(${screenScale.toFixed(4)})` }}>
              <div className="ove-screen__doc"><i/><i/><i/><i/><i/><i/></div>
              <div className="ove-screen__window" style={{ left: originX, top: originY, width: windowWidth, height: windowHeight }}>{scene}</div>
            </div>}
          </div>
          {view === "compose" && !drag?.moved && <RadiusHandle stageRef={stageRef} value={recipe.style.radius} label={radiusTitle} names={radiusNames}
            remeasure={`${composeScale}|${recipe.shell}|${layout}|${preferences.size}`} onPreview={setRadiusDraft}
            onCommit={(radius) => commitRecipe({ ...recipe, style: { ...recipe.style, radius } }, `${radiusTitle}: ${radiusNames[radius].toLowerCase()}`)}/>}
          {selected === "cancel" && view === "compose" && <CancelPopover recipe={recipe} stageRef={stageRef}
            onRecipe={(next, message) => commitRecipe(next, message)} onClose={() => setSelected(null)}/>}
          {typeof selected === "object" && selected && view === "compose" && SHELL_REGIONS[recipe.shell][selected.region] && !recipe.slots[selected.region]
            && <InsertPopover recipe={recipe} region={selected.region} stageRef={stageRef} names={names}
              onPick={(type, draw) => { if (apply(placeInto(recipe, type, draw, selected.region), t("{p0} → «{p1}»", { p0: names.elements[type], p1: names.regions[selected.region] }))) setSelected(null); }}
              onClose={() => setSelected(null)}/>}
          {typeof selected === "string" && selected !== "cancel" && view === "compose" && <Popover recipe={recipe} type={selected} stageRef={stageRef} names={names}
            onPick={(draw) => { const region = regionOf(recipe, selected); if (region) apply(placeInto(recipe, selected, draw, region), t("{p0}: «{p1}»", { p0: names.elements[selected], p1: names.draws[selected][draw] })); }}
            onRemove={() => { commitRecipe(removePart(recipe, selected), t("{p0} убран", { p0: names.elements[selected] })); setSelected(null); }}
            onMatrix={() => { setSelected(null); setTab("build"); requestAnimationFrame(() => document.getElementById("overlay-matrix-options")?.scrollIntoView({ block: "nearest", behavior: "smooth" })); }}
            onClose={() => setSelected(null)}/>}
        </div>
        <div className="ove-bar">
          <div className="ove-phases scroll-visible"><Segmented value={phaseMode} onChange={(value) => setPhase(value as PhaseMode)} options={PHASE_MODES.map((mode) => ({ value: mode, label: {
            scenario: t("Сценарий"), recording: t("Запись"), streaming: t("Стриминг"), processing: t("Обработка"),
            pasted: t("Вставка"), error: t("Ошибка"), limit: t("Лимит"),
          }[mode] }))}/></div>
        </div>
        {phaseMode === "streaming" && <p className="overlay-settings-hint">
          {t("Стриминг — запись с потоковой моделью: она показывает текст по ходу диктовки, не дожидаясь конца записи. С обычной моделью оверлей выглядит как в фазе «Запись».")}
          {" "}{regionOf(recipe, "draft")
            ? t("Текст появляется в черновике.")
            : t("В этом оверлее нет черновика, поэтому текст не виден. Добавьте его в «Сборке».")}
        </p>}
        <p className={`ove-toast${toast?.warn ? " ove-toast--warn" : ""}`} role="status" aria-live="polite">{toast?.text ?? (view === "compose" ? t("Перетащите деталь из библиотеки в область или нажмите её. Нажмите элемент в макете, чтобы сменить рисунок, или пустую область, чтобы что-то вставить.") : "")}</p>
        <AfterRecording recipe={recipe} preferences={preferences} names={names} place="stage"/>
      </div>
    </div>
    <AfterRecording recipe={recipe} preferences={preferences} names={names} place="page"/>
    {drag?.moved && createPortal(<div className="ove-ghost" style={{ left: drag.x + 14, top: drag.y + 14 }}>
      <span className="ove-ghost__pv" style={overlayPalette(preferences)}><PartPreview type={drag.type} draw={drag.draw ?? recipe.draw[drag.type]} recipe={recipe}/></span>
      <b>{names.elements[drag.type]}</b>
      {drag.remove ? <em>{t("Отпустите, чтобы убрать из корпуса")}</em> : drag.reason && <em>{drag.reason}</em>}
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
  const region = regionOf(recipe, type);
  const [ref, position] = usePopoverPlace(stageRef, region ? `[data-region="${region}"]` : null, recipe);
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
        className={`ove-part${isWidePart(type) ? " ove-part--wide" : ""}`}
        aria-pressed={recipe.draw[type] === draw} onClick={() => onPick(draw)}>
        <span className="ove-part__pv"><PartPreview type={type} draw={draw} recipe={recipe}/></span>
        <span className="ove-part__meta"><b>{names.draws[type][draw]}</b></span>
      </button>)}
    </div>
    {others.length > 0 && <p className="ove-pop__note">{t("Сюда не помещаются: {p0}.", { p0: others.join(", ") })}</p>}
    {type === "level" && recipe.draw.level === "matrix" && <button type="button" className="btn" onClick={onMatrix}>{t("Узор и плотность матрицы")} <Icon name="arrow-right" size={13}/></button>}
  </div>;
}

/** Everything that fits an empty region, grouped by element. */
function InsertPopover({ recipe, region, stageRef, names, onPick, onClose }: {
  recipe: Recipe; region: string; stageRef: RefObject<HTMLDivElement | null>; names: Names;
  onPick: (type: ElementType, draw: string) => void; onClose: () => void;
}) {
  const [ref, position] = usePopoverPlace(stageRef, `[data-region="${region}"]`, recipe);
  const parts = freePartsFor(recipe, region);
  return <div className="ove-pop ove-pop--insert" ref={ref} style={position} role="dialog" aria-label={t("{p0}: что вставить", { p0: names.regions[region] })}>
    <div className="ove-pop__head">
      <span><b>{names.regions[region]}</b> · {t("пусто")}</span>
      <button type="button" className="btn btn--ghost" aria-label={t("Закрыть")} onClick={onClose}><Icon name="x" size={13}/></button>
    </div>
    {parts.map(({ type, draws }) => {
      return <section className="ove-sec" key={type}>
        <header className="ove-sec__head"><b>{names.elements[type]}</b></header>
        <div className="ove-parts">
          {draws.map((draw) => <button key={draw} type="button" className={`ove-part${isWidePart(type) ? " ove-part--wide" : ""}`} onClick={() => onPick(type, draw)}>
            <span className="ove-part__pv"><PartPreview type={type} draw={draw} recipe={recipe}/></span>
            <span className="ove-part__meta"><b>{names.draws[type][draw]}</b></span>
          </button>)}
        </div>
      </section>;
    })}
    {parts.length === 0 && <p className="ove-pop__note">{partsFor(recipe, region).length
      ? t("Всё, что сюда помещается, уже в макете. Перетащите элемент сюда, чтобы переставить его.")
      : t("Сюда ничего не помещается.")}</p>}
  </div>;
}

/** Where a popover goes: under its target in the stage, above it only when the window has no room underneath. */
function usePopoverPlace(stageRef: RefObject<HTMLDivElement | null>, selector: string | null, recipe: Recipe) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<CSSProperties>({ visibility: "hidden" });
  useLayoutEffect(() => {
    const stage = stageRef.current, pop = ref.current;
    const target = selector ? stage?.querySelector<HTMLElement>(selector) : null;
    if (!stage || !pop || !target) return;
    const bounds = stage.getBoundingClientRect(), anchor = target.getBoundingClientRect();
    let top = anchor.bottom - bounds.top + 8;
    if (anchor.bottom + 8 + pop.offsetHeight > window.innerHeight && anchor.top - pop.offsetHeight - 8 > 0) top = anchor.top - bounds.top - pop.offsetHeight - 8;
    const left = Math.max(4, Math.min(bounds.width - pop.offsetWidth - 4, anchor.left - bounds.left + anchor.width / 2 - pop.offsetWidth / 2));
    setPosition({ top, left });
  }, [selector, recipe, stageRef]);
  return [ref, position] as const;
}

const cancelKept = () => t("Кнопку отмены нельзя убрать: без неё запись не остановить мышью. Её можно переставить или перерисовать.");

function CancelPopover({ recipe, stageRef, onRecipe, onClose }: {
  recipe: Recipe; stageRef: RefObject<HTMLDivElement | null>; onRecipe: (recipe: Recipe, message: string) => void; onClose: () => void;
}) {
  const [ref, position] = usePopoverPlace(stageRef, "[data-cancel]", recipe);
  return <div className="ove-pop" ref={ref} style={position} role="dialog" aria-label={t("Кнопка отмены: варианты")}>
    <div className="ove-pop__head">
      <span><b>{t("Отмена")}</b> · {cancelSpotNames()[recipe.cancel.at]}</span>
      <button type="button" className="btn btn--ghost" aria-label={t("Закрыть")} onClick={onClose}><Icon name="x" size={13}/></button>
    </div>
    <CancelOptions recipe={recipe} onRecipe={onRecipe}/>
    <p className="ove-pop__note">{cancelKept()}</p>
  </div>;
}

/** What the overlay does once the recording ends: the states the recipe does not choose. */
// Rendered twice: under the stage in a wide window, and under the library when the
// stage becomes a sticky strip in a narrow one. CSS shows one of them.
/** Mirrors `pasted_hold_ms` in overlay_preferences.rs, for the explanation under the stage. */
const PASTED_SECONDS = { short: 1, normal: 1.8, long: 3.5 } as const;

function AfterRecording({ recipe, preferences, names, place }: { recipe: Recipe; preferences: ReturnType<typeof preferencesOf>; names: Names; place: "stage" | "page" }) {
  const shell = recipe.shell, round = COMPACT_SHELLS.includes(shell), draft = regionOf(recipe, "draft");
  const pill = WINDOW_SIZE.pill[preferences.size].join("×"), stream = WINDOW_SIZE.streaming[preferences.size].join("×");
  const rows: Array<[string, string]> = [
    [t("Обработка"), t("«{p0}»; {p1}", { p0: processNames()[recipe.processing.draw], p1: round ? t("в центре корпуса")
      : shell === "card" ? t("статус встаёт в тело, остальное уходит") : shell === "caps" ? t("статус в чипе, по словам проходит блик")
      : shell === "island" ? t("строка сжимается под статус") : t("статус занимает всю строку") })],
    [t("Вставка"), t("{p0}; держится {p1} с", {
      p0: round || !recipe.pasted.words ? t("галочка в корпусе") : t("галочка и число вставленных символов"),
      p1: PASTED_SECONDS[recipe.pasted.hold].toLocaleString(localeTag()),
    })],
    [t("Ошибка"), round ? t("корпус раскрывается в пилюлю {p0}", { p0: pill }) : t("текст ошибки на месте статуса")],
    [t("Текст модели"), !draft ? t("не показывается: черновика нет") : draft === "below" ? t("окно переходит в {p0}, пока идёт стриминг", { p0: stream }) : t("в области «{p0}»", { p0: names.regions[draft] })],
    [t("Отсчёт лимита"), regionOf(recipe, "timer") ? t("таймер считает назад цветом предупреждения") : t("таймера нет, отсчёт заменит детали внутри корпуса")],
    [t("Отмена"), t("{p0}; видна: {p1}", { p0: cancelSpotNames()[recipe.cancel.at], p1: cancelShowNames()[recipe.cancel.show].toLowerCase() })],
  ];
  const warnings: string[] = [];
  if (!regionOf(recipe, "level") && !regionOf(recipe, "timer") && !regionOf(recipe, "rec")) warnings.push(t("Во время записи оверлей пуст: добавьте уровень, таймер или индикатор."));
  if (recipe.style.fill === "none" && (regionOf(recipe, "timer") || regionOf(recipe, "mode") || (draft && draft !== "lines"))) warnings.push(t("Без заливки цифры и текст теряются на светлом документе."));
  if (shell === "island" && !preferences.anchor.startsWith("top")) warnings.push(t("Остров задуман у верхнего края."));
  return <details className={`ove-after ove-after--${place}`} open>
    <summary>{t("После записи оверлей разложится так")}</summary>
    <dl>{rows.map(([term, text]) => <div key={term}><dt>{term}</dt><dd>{text}</dd></div>)}</dl>
    {warnings.length > 0 && <ul className="ove-warns">{warnings.map((text) => <li key={text}>{text}</li>)}</ul>}
  </details>;
}

