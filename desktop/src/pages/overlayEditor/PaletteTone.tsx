import { useRef, type CSSProperties } from "react";
import { t } from "../../i18n";
import { paletteTone } from "../../overlay/overlayPalette";
import type { OverlayPreferences } from "../../overlay/overlayPreferences";

type Palette = Pick<OverlayPreferences, "palette" | "palette_hue" | "palette_chroma">;
const RELEASE_KEYS = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"];
const MAX_CHROMA = 0.2;
/** Saturation a grey preset takes on when its hue is moved, so the move shows. */
const TINT_FROM_GREY = 0.1;

/** Hue and saturation of the overlay palette, the constructor's fine control under the swatches.
 *  They always show the tone in use; moving one turns a preset into the custom palette,
 *  starting from the preset's own tone. A drag reports every step through `onChange`
 *  and is saved once, through `onCommit`, on release. */
export function PaletteTone({ palette, onChange, onCommit }: {
  palette: Palette; onChange: (patch: Palette) => void; onCommit: (patch: Palette) => void;
}) {
  const tone = paletteTone(palette);
  // Focus and blur alone must not turn a preset into the custom palette.
  const moved = useRef(false);
  const commit = () => {
    if (!moved.current) return;
    moved.current = false;
    onCommit({ palette: "custom", ...tone });
  };
  const rows = [
    { key: "palette_hue" as const, label: t("Тон"), max: 359, step: 1, value: `${Math.round(tone.palette_hue)}°` },
    { key: "palette_chroma" as const, label: t("Насыщенность"), max: MAX_CHROMA, step: 0.005, value: `${Math.round(tone.palette_chroma / MAX_CHROMA * 100)}%` },
  ];
  // The tracks draw their own scale; the thumbs wear the colour picked.
  const style = { "--tone-h": tone.palette_hue, "--tone-c": tone.palette_chroma } as CSSProperties;
  return <div className="ove-tone" style={style}>
    {rows.map(({ key, label, max, step, value }) => <label className="ove-tone__cell" key={key}>
      <span className="ove-tone__head">{label}<output>{value}</output></span>
      <input type="range" className={`ove-tone__range ove-tone__range--${key === "palette_hue" ? "hue" : "chroma"}`}
        min="0" max={max} step={step} value={tone[key]}
        onChange={(event) => {
          moved.current = true;
          const next = { ...tone, [key]: Number(event.target.value) };
          if (key === "palette_hue" && next.palette_chroma === 0) next.palette_chroma = TINT_FROM_GREY;
          onChange({ palette: "custom", ...next });
        }}
        onPointerUp={commit}
        onKeyUp={(event) => { if (RELEASE_KEYS.includes(event.key)) commit(); }}
        onBlur={commit}/>
    </label>)}
  </div>;
}
