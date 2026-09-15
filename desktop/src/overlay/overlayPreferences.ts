export const OVERLAY_FORMS = ["pill", "bead"] as const;
export const OVERLAY_PALETTES = ["accent", "coal", "graphite", "lagoon", "amber", "violet", "custom"] as const;
export const OVERLAY_SIZES = ["s", "m", "l"] as const;
export const OVERLAY_ANCHORS = ["top-left", "top-center", "top-right", "center-left", "center", "center-right", "bottom-left", "bottom-center", "bottom-right"] as const;
export type OverlayPreferences = {
  form: typeof OVERLAY_FORMS[number];
  palette: typeof OVERLAY_PALETTES[number];
  palette_hue: number;
  palette_chroma: number;
  size: typeof OVERLAY_SIZES[number];
  anchor: typeof OVERLAY_ANCHORS[number];
  edge_offset: number;
};

function choice<T extends string>(values: readonly T[], value: unknown, fallback: T): T {
  return values.includes(value as T) ? value as T : fallback;
}
function number(value: unknown, min: number, max: number, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) && value >= min && value <= max ? value : fallback;
}
export function overlayPreferences(raw: unknown): OverlayPreferences {
  const value = raw && typeof raw === "object" ? raw as Record<string, unknown> : {};
  return {
    form: choice(OVERLAY_FORMS, value.form, "pill"),
    palette: choice(OVERLAY_PALETTES, value.palette, "accent"),
    palette_hue: typeof value.palette_hue === "number" && value.palette_hue < 360 ? number(value.palette_hue, 0, 360, 268) : 268,
    palette_chroma: number(value.palette_chroma, 0, 0.2, 0.14),
    size: choice(OVERLAY_SIZES, value.size, "m"),
    anchor: choice(OVERLAY_ANCHORS, value.anchor, "bottom-center"),
    edge_offset: Number.isInteger(value.edge_offset) ? number(value.edge_offset, 0, 512, 96) : 96,
  };
}

export type OverlayLayout = "pill" | "bead" | "streaming";
export function overlayLayout(form: OverlayPreferences["form"], streaming: boolean, needsText: boolean): OverlayLayout {
  if (needsText) return "pill";
  if (form === "bead") return "bead";
  return streaming ? "streaming" : "pill";
}
