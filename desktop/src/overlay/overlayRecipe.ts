import { MATRIX_DENSITIES, MATRIX_PROCESS, MATRIX_SPEECH, type MatrixDensity, type MatrixProcess, type MatrixSpeech } from "./dotMatrix";

// A recipe is what the overlay constructor builds: a shell, which element sits
// in each of its regions and how each is drawn, plus the style and motion.
// Colour, size and position stay the overlay's own preferences. The native
// side reads only the shell and whether the draft row is used (see
// overlay_preferences.rs); everything else is normalised here.

export const SHELLS = ["pill", "card", "bead", "stack", "island", "caps"] as const;
export type Shell = typeof SHELLS[number];
export type RegionKind = "small" | "wide" | "text" | "square" | "tall" | "edge";

export const SHELL_REGIONS: Record<Shell, Record<string, readonly RegionKind[]>> = {
  pill: { start: ["small"], center: ["wide"], end: ["small"], below: ["text"] },
  card: { body: ["text", "wide"], footL: ["small"], footR: ["small"], edge: ["edge"] },
  bead: { core: ["square"] },
  stack: { top: ["tall"], bottom: ["tall"] },
  island: { start: ["small"], center: ["wide"], end: ["small"], below: ["text"] },
  caps: { c1: ["small"], c2: ["small"], lines: ["text"] },
};

export const ELEMENTS = ["level", "timer", "rec", "mode", "draft"] as const;
export type ElementType = typeof ELEMENTS[number];

const ALL_SMALL: readonly RegionKind[] = ["small", "wide", "square", "tall"];
export const DRAWINGS: { [T in ElementType]: Record<string, readonly RegionKind[]> } = {
  level: {
    bars: ["small", "wide"], wave: ["small", "wide"], qbars: ["small", "wide"], scope: ["wide"],
    ascii: ["small", "wide"], caps: ALL_SMALL, matrix: ALL_SMALL, segments: ALL_SMALL,
    ring: ["square"], orb: ["small", "square", "tall"], beam: ["edge"],
  },
  timer: { capsule: ["small", "wide"], plain: ["small", "wide", "tall"], big: ["wide", "tall"] },
  rec: { dot: ["small", "wide", "tall", "square"], label: ["small", "wide"], REC: ["small", "wide", "tall"] },
  mode: { chip: ["wide"], short: ["small", "wide", "tall"] },
  draft: { tail: ["text"], plain: ["text"] },
};
export const DEFAULT_DRAW: Record<ElementType, string> = { level: "bars", timer: "capsule", rec: "dot", mode: "chip", draft: "tail" };

export const STYLE_OPTIONS = {
  radius: ["round", "soft", "sharp"],
  stroke: ["none", "hair", "rim"],
  fill: ["palette", "black", "none"],
  glow: ["0", "1", "2"],
  font: ["sans", "mono"],
} as const;
export type RecipeStyle = { [K in keyof typeof STYLE_OPTIONS]: typeof STYLE_OPTIONS[K][number] };
export const DEFAULT_STYLE: RecipeStyle = { radius: "round", stroke: "rim", fill: "palette", glow: "1", font: "sans" };

export const MOTIONS = ["quiet", "soft", "spring", "pixel"] as const;
export type Motion = typeof MOTIONS[number];

export type RecipeMatrix = { speech: MatrixSpeech; process: MatrixProcess; density: MatrixDensity };
export const DEFAULT_MATRIX: RecipeMatrix = { speech: "rings", process: "perimeter", density: 7 };

export type Recipe = {
  shell: Shell;
  slots: Record<string, ElementType | null>;
  draw: Record<ElementType, string>;
  style: RecipeStyle;
  motion: Motion;
  matrix: RecipeMatrix;
};
export type OverlaySize = "s" | "m" | "l";

export const compatible = (type: ElementType, draw: string, kinds: readonly RegionKind[]) =>
  (DRAWINGS[type][draw] ?? []).some((kind) => kinds.includes(kind));
export const firstKind = (type: ElementType, draw: string, kinds: readonly RegionKind[]) =>
  (DRAWINGS[type][draw] ?? []).find((kind) => kinds.includes(kind)) ?? kinds[0];
export const regionOf = (recipe: Recipe, type: ElementType) =>
  Object.keys(recipe.slots).find((region) => recipe.slots[region] === type) ?? null;
export const fitsShell = (type: ElementType, draw: string, shell: Shell) =>
  Object.values(SHELL_REGIONS[shell]).some((kinds) => compatible(type, draw, kinds));

export const emptySlots = (shell: Shell) =>
  Object.fromEntries(Object.keys(SHELL_REGIONS[shell]).map((region) => [region, null])) as Record<string, ElementType | null>;

function pick<T>(values: readonly T[], value: unknown, fallback: T): T {
  return values.includes(value as T) ? value as T : fallback;
}
const record = (value: unknown) => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};

/** A recipe from saved JSON, or null when there is none. Anything unknown or
 *  misplaced falls back one field at a time, so an old or hand-edited recipe
 *  still draws something close to what was saved. */
export function normalizeRecipe(raw: unknown): Recipe | null {
  const value = record(raw);
  if (!SHELLS.includes(value.shell as Shell)) return null;
  const shell = value.shell as Shell;
  const draw = { ...DEFAULT_DRAW };
  const rawDraw = record(value.draw);
  for (const type of ELEMENTS) if (typeof rawDraw[type] === "string" && DRAWINGS[type][rawDraw[type] as string]) draw[type] = rawDraw[type] as string;
  const slots = emptySlots(shell);
  const rawSlots = record(value.slots);
  const placed = new Set<ElementType>();
  for (const [region, kinds] of Object.entries(SHELL_REGIONS[shell])) {
    const type = rawSlots[region] as ElementType;
    if (!ELEMENTS.includes(type) || placed.has(type) || !compatible(type, draw[type], kinds)) continue;
    slots[region] = type;
    placed.add(type);
  }
  const rawStyle = record(value.style);
  const style = Object.fromEntries(Object.entries(STYLE_OPTIONS).map(([key, options]) =>
    [key, pick(options, rawStyle[key], DEFAULT_STYLE[key as keyof RecipeStyle])])) as RecipeStyle;
  const rawMatrix = record(value.matrix);
  return {
    shell, slots, draw, style,
    motion: pick(MOTIONS, value.motion, "soft"),
    matrix: {
      speech: pick(MATRIX_SPEECH, rawMatrix.speech, DEFAULT_MATRIX.speech),
      process: pick(MATRIX_PROCESS, rawMatrix.process, DEFAULT_MATRIX.process),
      density: pick(MATRIX_DENSITIES, rawMatrix.density, DEFAULT_MATRIX.density),
    },
  };
}

export type RecipeLayout = "pill" | "bead" | "streaming" | "glow" | "stack";
/** Mirrors `OverlayPreferences::layout` for a saved recipe in overlay_preferences.rs. */
export function recipeLayout(recipe: Recipe, streaming: boolean, needsText: boolean): RecipeLayout {
  if (recipe.shell === "card") return "glow";
  if (recipe.shell === "caps") return "streaming";
  if (needsText) return "pill";
  if (recipe.shell === "bead") return "bead";
  if (recipe.shell === "stack") return "stack";
  return streaming && recipe.slots.below === "draft" ? "streaming" : "pill";
}

/** Logical window sizes, mirroring `window_size` in overlay_preferences.rs. */
export const WINDOW_SIZE: Record<RecipeLayout, Record<OverlaySize, readonly [number, number]>> = {
  pill: { s: [280, 60], m: [308, 64], l: [360, 72] },
  streaming: { s: [520, 138], m: [600, 150], l: [680, 174] },
  glow: { s: [360, 100], m: [400, 112], l: [440, 124] },
  bead: { s: [64, 64], m: [72, 72], l: [80, 80] },
  stack: { s: [72, 112], m: [80, 128], l: [88, 144] },
};
/** The window a shell is designed around, before any state opens it wider. */
export const SHELL_LAYOUT: Record<Shell, RecipeLayout> = { pill: "pill", island: "pill", card: "glow", bead: "bead", stack: "stack", caps: "streaming" };
export const FONT_SCALE: Record<OverlaySize, number> = { s: 0.92, m: 1, l: 1.12 };

/** The legacy form that comes closest, kept in sync so an older Sotto that
 *  does not know recipes still shows a sensible overlay. */
export function nativeForm(shell: Shell): "pill" | "bead" | "glow" {
  return shell === "card" ? "glow" : shell === "bead" || shell === "stack" ? "bead" : "pill";
}

/**
 * The corner radius belongs to the shell, its size and the radius style. It
 * never depends on the state or on what the shell holds: a pill that opens
 * into the draft card and a bead that falls back to the pill row for an error
 * keep the same curve. CSS clamps a radius to half the short side, so a round
 * shell stays round whatever its current proportions.
 */
export function shellRadius(recipe: Recipe, size: OverlaySize) {
  if (recipe.style.radius === "sharp") return 3;
  if (recipe.style.radius === "soft") return Math.round(14 * FONT_SCALE[size]);
  if (recipe.shell === "card") return Math.round(28 * FONT_SCALE[size]);
  const [width, height] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][size];
  return Math.floor((Math.min(width, height) - 8) / 2);
}

export type ScenePhase = "recording" | "processing" | "pasted" | "error";
/** Shell size inside the window: the window minus a 4 px margin, except the
 *  island, which rests small and widens for what it has to say. */
export function shellSize(recipe: Recipe, size: OverlaySize, layout: RecipeLayout, phase: ScenePhase, shown: boolean): [number, number] {
  const [width, height] = WINDOW_SIZE[layout][size];
  if (recipe.shell === "island" && layout === "pill") {
    if (!shown && recipe.motion !== "quiet") return [120, 28];
    const scale = FONT_SCALE[size];
    if (phase === "processing") return [Math.round(200 * scale), height - 8];
    if (phase === "pasted") return [Math.round(256 * scale), height - 8];
  }
  return [width - 8, height - 8];
}

export const recipeHasMatrix = (recipe: Recipe) => recipe.draw.level === "matrix" && regionOf(recipe, "level") !== null;

export const MAX_TEMPLATES = 8;
export const TEMPLATE_NAME_MAX = 40;
export type UserTemplate = {
  id: string;
  name: string;
  recipe: Recipe;
  palette?: "graphite" | "copper" | "lagoon" | "violet" | "custom";
  palette_hue?: number;
  palette_chroma?: number;
  size?: OverlaySize;
};
const TEMPLATE_PALETTES = ["graphite", "copper", "lagoon", "violet", "custom"] as const;

/** The user's saved templates; a damaged entry is dropped rather than repaired. */
export function normalizeTemplates(raw: unknown): UserTemplate[] {
  if (!Array.isArray(raw)) return [];
  const templates: UserTemplate[] = [];
  for (const item of raw.slice(0, MAX_TEMPLATES)) {
    const value = record(item);
    const recipe = normalizeRecipe(value.recipe);
    const id = typeof value.id === "string" ? value.id.trim() : "";
    const name = typeof value.name === "string" ? value.name.trim() : "";
    if (!recipe || !id || !name || templates.some((template) => template.id === id)) continue;
    const template: UserTemplate = { id, name: name.slice(0, TEMPLATE_NAME_MAX), recipe };
    if (TEMPLATE_PALETTES.includes(value.palette as never)) template.palette = value.palette as UserTemplate["palette"];
    if (typeof value.palette_hue === "number" && value.palette_hue >= 0 && value.palette_hue < 360) template.palette_hue = value.palette_hue;
    if (typeof value.palette_chroma === "number" && value.palette_chroma >= 0 && value.palette_chroma <= 0.2) template.palette_chroma = value.palette_chroma;
    if (value.size === "s" || value.size === "m" || value.size === "l") template.size = value.size;
    templates.push(template);
  }
  return templates;
}
