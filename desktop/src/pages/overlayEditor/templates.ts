import { t } from "../../i18n";
import {
  DEFAULT_DRAW, DEFAULT_MATRIX, DEFAULT_STYLE, emptySlots, normalizeRecipe,
  type ElementType, type Motion, type Recipe, type RecipeMatrix, type RecipeStyle, type Shell,
} from "../../overlay/overlayRecipe";

function recipe(shell: Shell, slots: Record<string, ElementType>, draw: Partial<Recipe["draw"]>, style: Partial<RecipeStyle>, motion: Motion, matrix: Partial<RecipeMatrix> = {}): Recipe {
  return {
    shell, slots: { ...emptySlots(shell), ...slots }, draw: { ...DEFAULT_DRAW, ...draw },
    style: { ...DEFAULT_STYLE, ...style }, motion, matrix: { ...DEFAULT_MATRIX, ...matrix },
  };
}

const flat = { stroke: "hair", fill: "black", glow: "0" } as const;

/** Sotto's own starting points. The first four are the quick choices in the
 *  overlay card and are meant to look clearly different from each other. */
export const SYSTEM_TEMPLATES = {
  pill: recipe("pill", { start: "timer", center: "level", below: "draft" }, {}, {}, "soft"),
  bead: recipe("bead", { core: "level" }, { level: "ring" }, {}, "soft"),
  glow: recipe("card", { body: "draft", footL: "timer", edge: "level" }, { level: "beam", timer: "plain" }, { glow: "0" }, "soft"),
  orb: recipe("bead", { core: "level" }, { level: "orb" }, { stroke: "none", fill: "none", glow: "0" }, "soft"),
  plank: recipe("pill", { start: "timer", center: "level", below: "draft" }, { level: "qbars", timer: "plain" }, { radius: "sharp", ...flat }, "pixel"),
  square: recipe("bead", { core: "level" }, { level: "matrix" }, { radius: "sharp", ...flat }, "pixel", { speech: "rings", process: "perimeter", density: 7 }),
  stack: recipe("stack", { top: "level", bottom: "timer" }, { level: "matrix", timer: "plain" }, flat, "soft", { speech: "ripple", process: "sonar", density: 9 }),
  scope: recipe("pill", { start: "timer", center: "level" }, { level: "scope", timer: "plain" }, { radius: "soft", ...flat }, "soft"),
  caps: recipe("caps", { c1: "timer", c2: "level", lines: "draft" }, { timer: "plain" }, { stroke: "none", fill: "black", glow: "0" }, "soft"),
  term: recipe("pill", { start: "rec", center: "level", end: "timer", below: "draft" }, { rec: "REC", level: "ascii", timer: "plain" }, { radius: "soft", ...flat, font: "mono" }, "pixel"),
} satisfies Record<string, Recipe>;
export type SystemTemplate = keyof typeof SYSTEM_TEMPLATES;
export const QUICK_TEMPLATES: SystemTemplate[] = ["pill", "bead", "glow", "orb"];

export const systemTemplateNames = (): Record<SystemTemplate, string> => ({
  pill: t("Пилюля"), bead: t("Бусина"), glow: t("Сияние"), orb: t("Сфера"), plank: t("Планка"), square: t("Квадрат"),
  stack: t("Стопка"), scope: t("Осциллограф"), caps: t("Субтитры"), term: t("Терминал"),
});

/** The recipe a config without one is drawn with, so the constructor opens on what the user already sees. */
export function recipeForForm(form: "pill" | "bead" | "glow"): Recipe {
  return structuredClone(SYSTEM_TEMPLATES[form]);
}

/** Compared through normalisation, so key order and absent empty regions do not count as a change. */
export const sameRecipe = (a: Recipe, b: Recipe) => JSON.stringify(normalizeRecipe(a)) === JSON.stringify(normalizeRecipe(b));
