import { useEffect, useRef, useState } from "react";
import { Card, Segmented } from "../components/Shell";
import { CustomSelect } from "../components/CustomSelect";
import { NumberField } from "../components/NumberField";
import { ACCENT_OPTIONS, resolveAccent } from "../accent";
import type { ConfigResult } from "../bridge/types";
import { t } from "../i18n";
import { OVERLAY_ANCHORS, overlayPreferences, type OverlayPreferences } from "../overlay/overlayPreferences";
import { overlayPalette } from "../overlay/overlayPalette";

type Props = {
  config: ConfigResult | null;
  onConfigChanged: (patch: Partial<ConfigResult>) => Promise<ConfigResult | null>;
};

export function OverlaySettings({ config, onConfigChanged }: Props) {
  const [draft, setDraft] = useState(() => overlayPreferences(config?.overlay));
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [error, setError] = useState("");
  useEffect(() => { setDraft(overlayPreferences(config?.overlay)); }, [config?.overlay]);

  async function save(patch: Partial<OverlayPreferences>, accent?: ConfigResult["ui_accent"]) {
    if (saving.current) return;
    const saved = overlayPreferences(config?.overlay);
    if (!accent && Object.entries(patch).every(([key, value]) => saved[key as keyof OverlayPreferences] === value)) return;
    saving.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await onConfigChanged(accent ? { ui_accent: accent } : { overlay: patch });
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
    { value: "accent", label: t("Акцент приложения") }, { value: "coal", label: t("Уголь") },
    { value: "graphite", label: t("Графит") }, { value: "lagoon", label: t("Лагуна") },
    { value: "amber", label: t("Янтарь") }, { value: "violet", label: t("Фиолетовый") },
    { value: "custom", label: t("Своя палитра") },
  ];
  const anchorLabels = [t("Сверху слева"), t("Сверху по центру"), t("Сверху справа"),
    t("Слева по центру"), t("По центру"), t("Справа по центру"),
    t("Снизу слева"), t("Снизу по центру"), t("Снизу справа")];
  return <Card pad="rows" className="overlay-settings">
    <section aria-label={t("Оверлей")} data-testid="overlay-settings">
      <h2 className="section-title">{t("Оверлей")}</h2>
      <div className="overlay-settings-grid">
        <div className="set-cell" role="group" aria-label={t("Форма")}>
          <span className="set-label">{t("Форма")}</span>
          <Segmented value={draft.form} disabled={busy || !config} options={[
            { value: "pill", label: t("Пилюля") }, { value: "bead", label: t("Бусина") },
          ]} onChange={(form) => void save({ form: form as OverlayPreferences["form"] })}/>
        </div>
        <div className="set-cell" role="group" aria-label={t("Размер")}>
          <span className="set-label">{t("Размер")}</span>
          <Segmented value={draft.size} disabled={busy || !config} options={[
            { value: "s", label: "S" }, { value: "m", label: "M" }, { value: "l", label: "L" },
          ]} onChange={(size) => void save({ size: size as OverlayPreferences["size"] })}/>
        </div>
        <div className="set-cell" role="group" aria-label={t("Палитра")}>
          <span className="set-label">{t("Палитра")}</span>
          <CustomSelect value={draft.palette} options={paletteOptions} disabled={busy || !config}
            onChange={(palette) => void save({ palette })}/>
        </div>
        {draft.palette === "accent" && <div className="set-cell" role="group" aria-label={t("Акцент приложения")}>
          <span className="set-label">{t("Акцент приложения")}</span>
          <CustomSelect value={resolveAccent(config?.ui_accent)} options={[...ACCENT_OPTIONS()]}
            disabled={busy || !config} onChange={(accent) => void save({}, accent)}/>
        </div>}
      </div>
      {draft.form === "bead" && <p className="overlay-settings-hint" data-testid="bead-hint">
        {t("Для отмены наведите указатель на бусину.")} {t("В бусине потоковый текст не отображается.")}
      </p>}
      {draft.palette === "custom" && <div className="overlay-settings-grid">
        {(["palette_hue", "palette_chroma"] as const).map((key) => <label className="set-cell" key={key}>
          <span className="set-label">{key === "palette_hue" ? t("Тон") : t("Насыщенность")}</span>
          <input type="range" min="0" max={key === "palette_hue" ? 359 : 0.2} step={key === "palette_hue" ? 1 : 0.005}
            value={draft[key]} disabled={busy || !config}
            onChange={(event) => setDraft({ ...draft, [key]: Number(event.target.value) })}
            onPointerUp={() => void save({ [key]: draft[key] })}
            onKeyUp={(event) => { if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"].includes(event.key)) void save({ [key]: draft[key] }); }}
            onBlur={() => void save({ [key]: draft[key] })}/>
        </label>)}
      </div>}
      <div className="overlay-palette-preview" aria-hidden="true" style={overlayPalette(draft, config?.ui_accent)}><span/></div>
      <div className="overlay-settings-grid">
        <div className="set-cell" role="group" aria-label={t("Положение на экране")}>
          <span className="set-label">{t("Положение на экране")}</span>
          <div className="overlay-anchor-grid">
            {OVERLAY_ANCHORS.map((anchor, index) => <button type="button" className="btn btn--ghost"
              key={anchor} aria-label={anchorLabels[index]} aria-pressed={draft.anchor === anchor}
              disabled={busy || !config} onClick={() => void save({ anchor })}>
              {["↖", "↑", "↗", "←", "·", "→", "↙", "↓", "↘"][index]}
            </button>)}
          </div>
        </div>
        <div className="set-cell">
          <label className="set-label" htmlFor="overlay-offset">{t("Отступ от края")}</label>
          <NumberField id="overlay-offset" min={0} max={512} step={1} value={draft.edge_offset}
            disabled={busy || !config || draft.anchor === "center"}
            onValueChange={(value) => setDraft({ ...draft, edge_offset: Number(value) })}
            onStepCommit={(value) => void save({ edge_offset: Number(value) })}
            onBlur={() => {
              if (Number.isInteger(draft.edge_offset) && draft.edge_offset >= 0 && draft.edge_offset <= 512) void save({ edge_offset: draft.edge_offset });
              else setDraft(overlayPreferences(config?.overlay));
            }}/>
        </div>
      </div>
      {error && <p className="overlay-settings-error" role="alert">{error}</p>}
    </section>
  </Card>;
}
