import { useRef, useState } from "react";
import type { ConfigResult } from "../../bridge/types";
import { t } from "../../i18n";
import { nativeForm, type Recipe, type UserTemplate } from "../../overlay/overlayRecipe";
import { overlayPreferences, type OverlayPreferences } from "../../overlay/overlayPreferences";
import { replacementPatch } from "./recipeEdits";
import { recipeForForm, sameRecipe, SYSTEM_TEMPLATES, type SystemTemplate } from "./templates";

export type OverlayLook = Pick<OverlayPreferences, "palette" | "palette_hue" | "palette_chroma" | "size" | "anchor" | "edge_offset">;
export type ConfigChange = (patch: Partial<ConfigResult>) => Promise<ConfigResult | null>;

/** What the overlay draws now: the saved recipe, or the one matching the legacy form. */
export const currentRecipe = (preferences: OverlayPreferences) => preferences.recipe ?? recipeForForm(preferences.form);

/** Which template, if any, the current overlay matches exactly. */
export function matchingTemplate(recipe: Recipe, preferences: OverlayPreferences): { kind: "system"; key: SystemTemplate } | { kind: "mine"; template: UserTemplate } | null {
  const mine = preferences.templates.find((template) => sameRecipe(template.recipe, recipe)
    && (template.palette ?? preferences.palette) === preferences.palette && (template.size ?? preferences.size) === preferences.size);
  if (mine) return { kind: "mine", template: mine };
  const key = (Object.keys(SYSTEM_TEMPLATES) as SystemTemplate[]).find((name) => sameRecipe(SYSTEM_TEMPLATES[name], recipe));
  return key ? { kind: "system", key } : null;
}

/**
 * Saving for the overlay card and the constructor. Changes go out one at a
 * time in the order they were made, so quick clicks never land out of
 * order, and a recipe is sent as a patch that also clears the regions the
 * new shell does not have.
 */
export function useOverlaySaver(config: ConfigResult | null, onConfigChanged: ConfigChange) {
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const savedRecipe = useRef<unknown>(config?.overlay?.recipe ?? null);
  const [error, setError] = useState("");

  function save(patch: Record<string, unknown>) {
    queue.current = queue.current.then(async () => {
      try {
        const result = await onConfigChanged({ overlay: patch } as Partial<ConfigResult>);
        setError(result ? "" : t("Не удалось сохранить настройки оверлея. Попробуйте ещё раз."));
      } catch {
        setError(t("Не удалось сохранить настройки оверлея. Попробуйте ещё раз."));
      }
    });
    return queue.current;
  }
  function saveRecipe(recipe: Recipe, extra: Partial<OverlayLook> = {}) {
    const patch = { ...extra, recipe: replacementPatch(savedRecipe.current, recipe), form: nativeForm(recipe.shell) };
    savedRecipe.current = recipe;
    return save(patch);
  }
  function saveTemplates(templates: UserTemplate[]) {
    return save({ templates });
  }
  return { save, saveRecipe, saveTemplates, error };
}

export const preferencesOf = (config: ConfigResult | null) => overlayPreferences(config?.overlay);

/** The colour and size a user template brings with it. */
export function templateLook(template: UserTemplate): Partial<OverlayLook> {
  const look: Partial<OverlayLook> = {};
  if (template.palette) look.palette = template.palette;
  if (template.palette_hue !== undefined) look.palette_hue = template.palette_hue;
  if (template.palette_chroma !== undefined) look.palette_chroma = template.palette_chroma;
  if (template.size) look.size = template.size;
  return look;
}

export function newTemplate(name: string, recipe: Recipe, preferences: OverlayPreferences): UserTemplate {
  return {
    id: Date.now().toString(36), name, recipe: structuredClone(recipe),
    palette: preferences.palette, palette_hue: preferences.palette_hue, palette_chroma: preferences.palette_chroma, size: preferences.size,
  };
}
