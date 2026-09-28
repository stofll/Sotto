import {
  DEFAULT_STYLE, DRAWINGS, SHELL_REGIONS, compatible, emptySlots, fitCancel, fitsShell, regionOf,
  type ElementType, type Recipe, type RegionKind, type Shell,
} from "../../overlay/overlayRecipe";

// Every edit the constructor makes to a recipe. Each returns a new recipe
// or a reason it cannot be done; nothing here knows about the screen.

const hasOrb = (recipe: Recipe) => recipe.draw.level === "orb" && regionOf(recipe, "level") !== null;

/** The orb is the only drawing meant to float without a shell. When an edit
 *  takes it away, a shell that is still invisible gets its default look back,
 *  otherwise the new parts would hang in the air. */
export function restoreShell(before: Recipe, after: Recipe): Recipe {
  if (!hasOrb(before) || hasOrb(after) || after.style.fill !== "none" || after.style.stroke !== "none") return after;
  return { ...after, style: { ...after.style, fill: DEFAULT_STYLE.fill, stroke: DEFAULT_STYLE.stroke, glow: DEFAULT_STYLE.glow } };
}

/** Where an element goes first when it is added without choosing a place. */
const PREFERRED: Record<Shell, Partial<Record<ElementType, string[]>>> = {
  pill: { level: ["center", "start", "end"], timer: ["start", "end", "center"], rec: ["start", "end", "center"], mode: ["end", "center", "start"], draft: ["below"] },
  island: { level: ["center", "start", "end"], timer: ["start", "end", "center"], rec: ["start", "end", "center"], mode: ["end", "center", "start"], draft: ["below"] },
  card: { level: ["edge", "body", "footR", "footL"], timer: ["footL", "footR", "body"], rec: ["footL", "footR", "body"], mode: ["footR", "footL", "body"], draft: ["body"] },
  bead: { level: ["core"], rec: ["core"] },
  stack: { level: ["top", "bottom"], timer: ["bottom", "top"], rec: ["top", "bottom"], mode: ["bottom", "top"] },
  caps: { draft: ["lines"], timer: ["c1", "c2"], level: ["c2", "c1"], rec: ["c1", "c2"], mode: ["c2", "c1"] },
  mini: { level: ["start", "end"], timer: ["end", "start"], rec: ["start", "end"], mode: ["end", "start"] },
};
/** Placement order when a new shell has to take everything the old one held: the text first, it has one place. */
const PLACEMENT_ORDER: ElementType[] = ["draft", "level", "timer", "rec", "mode"];

const clone = (recipe: Recipe): Recipe => structuredClone(recipe);
const pickDraw = (type: ElementType, current: string, kinds: readonly RegionKind[]) =>
  compatible(type, current, kinds) ? current : Object.keys(DRAWINGS[type]).find((draw) => compatible(type, draw, kinds)) ?? null;

export type EditResult =
  | { ok: true; recipe: Recipe; switched?: ElementType[]; left?: ElementType[] }
  | { ok: false; reason: "no-fit" | "no-room"; type: ElementType; draw: string; region?: string };

/** Put `type` into `region`, drawn as `draw` or the nearest drawing that fits.
 *  Whatever sat there moves to where `type` came from when it fits, or leaves.
 *  `switched` names the elements redrawn without being asked. */
export function placeInto(recipe: Recipe, type: ElementType, draw: string | null, region: string): EditResult {
  const kinds = SHELL_REGIONS[recipe.shell][region];
  if (!kinds) return { ok: false, reason: "no-fit", type, draw: draw ?? recipe.draw[type], region };
  const chosen = draw ? (compatible(type, draw, kinds) ? draw : null) : pickDraw(type, recipe.draw[type], kinds);
  if (!chosen) return { ok: false, reason: "no-fit", type, draw: draw ?? recipe.draw[type], region };
  const next = clone(recipe);
  const from = regionOf(next, type);
  const displaced = next.slots[region];
  const switched: ElementType[] = !draw && chosen !== recipe.draw[type] ? [type] : [];
  if (from) next.slots[from] = null;
  next.slots[region] = type;
  next.draw[type] = chosen;
  if (displaced && displaced !== type && from) {
    const back = pickDraw(displaced, next.draw[displaced], SHELL_REGIONS[next.shell][from]);
    if (back) { next.slots[from] = displaced; next.draw[displaced] = back; }
    if (back && back !== recipe.draw[displaced]) switched.push(displaced);
  }
  return { ok: true, recipe: next, switched };
}

/** A click on a part: redraw the element where it already is, or put it in the best free place. */
export function addPart(recipe: Recipe, type: ElementType, draw: string): EditResult {
  const current = regionOf(recipe, type);
  if (current && compatible(type, draw, SHELL_REGIONS[recipe.shell][current])) {
    const next = clone(recipe);
    next.draw[type] = draw;
    return { ok: true, recipe: next };
  }
  if (!fitsShell(type, draw, recipe.shell)) return { ok: false, reason: "no-fit", type, draw };
  const regions = SHELL_REGIONS[recipe.shell];
  const candidates = [...(PREFERRED[recipe.shell][type] ?? []), ...Object.keys(regions)]
    .filter((region, index, all) => all.indexOf(region) === index && compatible(type, draw, regions[region]));
  const free = candidates.find((region) => !recipe.slots[region]);
  if (free) return placeInto(recipe, type, draw, free);
  if (current) return placeInto(recipe, type, draw, candidates[0]);
  return { ok: false, reason: "no-room", type, draw };
}

export function removePart(recipe: Recipe, type: ElementType): Recipe {
  const next = clone(recipe);
  const region = regionOf(next, type);
  if (region) next.slots[region] = null;
  return next;
}

/** Move to another shell and carry over what fits, redrawing an element when
 *  its drawing has no place there. `switched` and `left` say what changed. */
export function changeShell(recipe: Recipe, shell: Shell): EditResult & { ok: true } {
  const next = clone(recipe);
  next.shell = shell;
  next.slots = emptySlots(shell);
  next.cancel = fitCancel(shell, recipe.cancel);
  const regions = SHELL_REGIONS[shell];
  const placed = PLACEMENT_ORDER.filter((type) => regionOf(recipe, type));
  const switched: ElementType[] = [];
  // First pass keeps drawings as they are; the second lets an element change its drawing to fit.
  for (const redraw of [false, true]) {
    for (const type of placed) {
      if (regionOf(next, type)) continue;
      const wanted = [regionOf(recipe, type), ...(PREFERRED[shell][type] ?? []), ...Object.keys(regions)]
        .filter((region): region is string => !!region && !!regions[region] && !next.slots[region]);
      const region = wanted.find((candidate) => redraw ? pickDraw(type, next.draw[type], regions[candidate]) : compatible(type, next.draw[type], regions[candidate]));
      if (!region) continue;
      const draw = pickDraw(type, next.draw[type], regions[region])!;
      if (draw !== next.draw[type]) switched.push(type);
      next.draw[type] = draw;
      next.slots[region] = type;
    }
  }
  return { ok: true, recipe: next, switched, left: placed.filter((type) => !regionOf(next, type)) };
}

/** The order parts are offered in, in the library and in an empty region. */
export const PART_ORDER: ElementType[] = ["level", "timer", "rec", "draft", "mode"];

/** What an empty region can take: each element with the drawings that fit there. */
export function partsFor(recipe: Recipe, region: string): Array<{ type: ElementType; draws: string[] }> {
  const kinds = SHELL_REGIONS[recipe.shell][region];
  if (!kinds) return [];
  return PART_ORDER
    .map((type) => ({ type, draws: Object.keys(DRAWINGS[type]).filter((draw) => compatible(type, draw, kinds)) }))
    .filter((part) => part.draws.length > 0);
}

/** What an empty region can take that is not in the shell yet: a placed part is moved by dragging it. */
export const freePartsFor = (recipe: Recipe, region: string) => partsFor(recipe, region).filter((part) => !regionOf(recipe, part.type));

/** The kinds of place a drawing can go, for "fits: …" hints. */
export const drawingKinds = (type: ElementType, draw: string) => DRAWINGS[type][draw] ?? [];

/**
 * A JSON merge patch that turns `before` into `after`. The config is saved
 * with RFC 7396 merge semantics, where an object merges key by key; without
 * the explicit nulls a region the new shell does not have would stay behind
 * in the saved recipe.
 */
export function replacementPatch(before: unknown, after: unknown): unknown {
  if (!after || typeof after !== "object" || Array.isArray(after) || !before || typeof before !== "object" || Array.isArray(before)) return after;
  const patch: Record<string, unknown> = {};
  const old = before as Record<string, unknown>, next = after as Record<string, unknown>;
  for (const key of Object.keys(old)) if (!(key in next)) patch[key] = null;
  for (const [key, value] of Object.entries(next)) patch[key] = replacementPatch(old[key], value);
  return patch;
}
