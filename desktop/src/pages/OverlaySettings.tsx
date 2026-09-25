import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Segmented } from "../components/Shell";
import { NumberField } from "../components/NumberField";
import { Hint } from "../components/Hint";
import { Icon } from "../components/Icon";
import type { ConfigResult } from "../bridge/types";
import { t } from "../i18n";
import { DEFAULT_EDGE_OFFSET, OVERLAY_ANCHORS, overlayPreferences, type OverlayPreferences } from "../overlay/overlayPreferences";
import { overlayPalette } from "../overlay/overlayPalette";

type Props = {
  config: ConfigResult | null;
  onConfigChanged: (patch: Partial<ConfigResult>) => Promise<ConfigResult | null>;
};

// Fixed bar heights: the preview is a still picture of the overlay, not a
// second waveform. Animating it would compete for the eye with the control the
// user is actually holding.
const PREVIEW_BARS = [7, 12, 18, 24, 16, 21, 28, 19, 13, 22, 26, 17, 11, 20, 25, 14, 9, 16, 12, 8];
// A schematic display in logical pixels. Scale the entire scene so window
// geometry and edge offsets always use the same coordinate system.
const PREVIEW_WIDTH = 1920;
const PREVIEW_HEIGHT = 1080;

function previewPlacement(anchor: OverlayPreferences["anchor"], edgeOffset: number): CSSProperties {
  // «center» is the one anchor written as a single word.
  const [vertical, horizontal] = anchor === "center" ? ["center", "center"] : anchor.split("-");
  const inset = Math.max(0, Math.min(edgeOffset, 512));
  const shift = (axis: string) => axis === "center" ? "-50%" : "0";
  return {
    top: vertical === "center" ? "50%" : vertical === "top" ? inset : undefined,
    bottom: vertical === "bottom" ? inset : undefined,
    left: horizontal === "center" ? "50%" : horizontal === "left" ? inset : undefined,
    right: horizontal === "right" ? inset : undefined,
    transform: `translate(${shift(horizontal)}, ${shift(vertical)})`,
  };
}

export function OverlaySettings({ config, onConfigChanged }: Props) {
  const [draft, setDraft] = useState(() => overlayPreferences(config?.overlay));
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [error, setError] = useState("");
  useEffect(() => { setDraft(overlayPreferences(config?.overlay)); }, [config?.overlay]);

  const screenRef = useRef<HTMLDivElement>(null);
  const [previewScale, setPreviewScale] = useState(0);
  useEffect(() => {
    const screen = screenRef.current;
    if (!screen) return;
    const observer = new ResizeObserver(([entry]) => {
      setPreviewScale(entry.contentRect.width / PREVIEW_WIDTH);
    });
    observer.observe(screen);
    return () => observer.disconnect();
  }, []);

  async function save(patch: Partial<OverlayPreferences>) {
    if (saving.current) return;
    const saved = overlayPreferences(config?.overlay);
    if (Object.entries(patch).every(([key, value]) => saved[key as keyof OverlayPreferences] === value)) return;
    saving.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await onConfigChanged({ overlay: patch });
      setDraft(overlayPreferences(result?.overlay ?? config?.overlay));
      if (!result) setError(t("Не удалось сохранить настройки оверлея. Попробуйте ещё раз."));
    } catch {
      setDraft(saved);
      setError(t("Не удалось сохранить настройки оверлея. Попробуйте ещё раз."));
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  const paletteOptions: Array<{ value: OverlayPreferences["palette"]; label: string }> = [
    { value: "graphite", label: t("Графит") }, { value: "copper", label: t("Медь") },
    { value: "lagoon", label: t("Лагуна") }, { value: "violet", label: t("Фиолетовый") },
    { value: "custom", label: t("Своя палитра") },
  ];
  const anchorLabels = [t("Сверху слева"), t("Сверху по центру"), t("Сверху справа"),
    t("Слева по центру"), t("По центру"), t("Справа по центру"),
    t("Снизу слева"), t("Снизу по центру"), t("Снизу справа")];
  const locked = busy || !config;
  return <section className="overlay-settings" aria-label={t("Оверлей")} data-testid="overlay-settings">
    <div className="overlay-settings-grid">
      <div className="set-cell set-cell--auto" role="group" aria-label={t("Форма")}>
        <span className="set-label">{t("Форма")}</span>
        <Segmented value={draft.form} disabled={locked} options={[
          { value: "pill", label: t("Пилюля") }, { value: "bead", label: t("Бусина") },
          { value: "glow", label: t("Свечение") },
        ]} onChange={(form) => void save({ form: form as OverlayPreferences["form"] })}/>
      </div>
      <div className="set-cell set-cell--auto" role="group" aria-label={t("Размер")}>
        <span className="set-label">{t("Размер")}</span>
        <Segmented value={draft.size} disabled={locked} options={[
          { value: "s", label: "S" }, { value: "m", label: "M" }, { value: "l", label: "L" },
        ]} onChange={(size) => void save({ size: size as OverlayPreferences["size"] })}/>
      </div>
      <div className="set-cell set-cell--auto overlay-offset-cell">
        <label className="set-label" htmlFor="overlay-offset">{t("Отступ от края")}</label>
        <div className="overlay-offset-row">
          <NumberField id="overlay-offset" min={0} max={512} step={1} value={draft.edge_offset}
            disabled={locked || draft.anchor === "center"}
            onValueChange={(value) => setDraft({ ...draft, edge_offset: Number(value) })}
            onStepCommit={(value) => void save({ edge_offset: Number(value) })}
            onBlur={() => {
              if (Number.isInteger(draft.edge_offset) && draft.edge_offset >= 0 && draft.edge_offset <= 512) void save({ edge_offset: draft.edge_offset });
              else setDraft(overlayPreferences(config?.overlay));
            }}/>
          <Hint text={t("Вернуть отступ по умолчанию — {p0} px", { p0: DEFAULT_EDGE_OFFSET })}>
            <button type="button" className="btn btn--ghost" aria-label={t("Сбросить отступ")}
              disabled={locked || draft.anchor === "center" || draft.edge_offset === DEFAULT_EDGE_OFFSET}
              onClick={() => { setDraft({ ...draft, edge_offset: DEFAULT_EDGE_OFFSET }); void save({ edge_offset: DEFAULT_EDGE_OFFSET }); }}>
              <Icon name="refresh" size={14}/>
            </button>
          </Hint>
        </div>
      </div>
      <div className="set-cell set-cell--auto overlay-timer-cell" role="group" aria-label={t("Секундомер")}>
        <label className="checkbox-row"><input type="checkbox" className="checkbox" checked={draft.show_timer} disabled={locked} onChange={(event) => void save({ show_timer: event.target.checked })}/>{t("Секундомер")}</label>
      </div>
      <div className="set-cell set-cell--auto" role="group" aria-label={t("Цвет оверлея")}>
        <span className="set-label">{t("Цвет оверлея")}</span>
        {/* The names live on the swatches themselves: a caption under the row
            claimed a line of its own to repeat what hovering already tells. */}
        <div className="overlay-swatches">
          {paletteOptions.map(({ value, label }) => <Hint key={value} text={label}>
            <button type="button"
              className={`overlay-swatch${value === "custom" ? " overlay-swatch--custom" : ""}`}
              aria-label={label} aria-pressed={draft.palette === value} disabled={locked}
              style={overlayPalette({ ...draft, palette: value })}
              onClick={() => void save({ palette: value })}/>
          </Hint>)}
        </div>
      </div>
    </div>
    {draft.form === "bead" && <p className="overlay-settings-hint" data-testid="bead-hint">
      {t("Для отмены наведите указатель на бусину.")} {t("В бусине потоковый текст не отображается.")}
    </p>}
    {draft.palette === "custom" && <div className="overlay-settings-grid">
      {(["palette_hue", "palette_chroma"] as const).map((key) => <label className="set-cell" key={key}>
        <span className="set-label">{key === "palette_hue" ? t("Тон") : t("Насыщенность")}</span>
        <input type="range" min="0" max={key === "palette_hue" ? 359 : 0.2} step={key === "palette_hue" ? 1 : 0.005}
          value={draft[key]} disabled={locked}
          onChange={(event) => setDraft({ ...draft, [key]: Number(event.target.value) })}
          onPointerUp={() => void save({ [key]: draft[key] })}
          onKeyUp={(event) => { if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"].includes(event.key)) void save({ [key]: draft[key] }); }}
          onBlur={() => void save({ [key]: draft[key] })}/>
      </label>)}
    </div>}
    <div className="set-cell overlay-place-cell" role="group" aria-label={t("Положение на экране")}>
      <span className="set-label">{t("Положение на экране")}
        <Hint text={t("Щёлкните по месту на экране, где должен появляться оверлей.")}/>
      </span>
      <div className="overlay-screen" data-testid="overlay-preview" ref={screenRef}
        style={{ ...overlayPalette(draft), aspectRatio: `${PREVIEW_WIDTH} / ${PREVIEW_HEIGHT}` }}>
        {OVERLAY_ANCHORS.map((anchor, index) => <button type="button" key={anchor}
          className="overlay-screen__zone" aria-label={anchorLabels[index]}
          aria-pressed={draft.anchor === anchor} disabled={locked}
          onClick={() => void save({ anchor })}/>)}
        <div className="overlay-screen__scene" aria-hidden="true"
          style={{ width: PREVIEW_WIDTH, height: PREVIEW_HEIGHT, transform: `scale(${previewScale})` }}>
          <span className="overlay-mini" data-form={draft.form} data-size={draft.size}
            data-anchor={draft.anchor}
            style={{ ...overlayPalette(draft), ...previewPlacement(draft.anchor, draft.edge_offset) }}>
            <span className="overlay-mini__shell">
              <span className="overlay-mini__surface">
                {draft.form === "bead" ? <span className="overlay-mini__ring"/> : draft.form === "glow" ? <>
                  <span className="overlay-mini__copy"/>
                  <span className="overlay-mini__bar">
                    {draft.show_timer && <span className="overlay-mini__timer">0:07</span>}
                    <span className="overlay-mini__close"><Icon name="x" size={14}/></span>
                  </span>
                </> : <>
                  {draft.show_timer && <span className="overlay-mini__timer">0:07</span>}
                  <span className="overlay-mini__wave">
                    {PREVIEW_BARS.map((height, bar) => <span key={bar} style={{ height }}/>)}
                  </span>
                  <span className="overlay-mini__close"><Icon name="x" size={14}/></span>
                </>}
              </span>
            </span>
          </span>
        </div>
      </div>
    </div>
    {error && <p className="overlay-settings-error" role="alert">{error}</p>}
  </section>;
}
