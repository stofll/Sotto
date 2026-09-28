import { describe, expect, it } from "vitest";
import { overlayPreferences } from "../../overlay/overlayPreferences";
import { matchingTemplate, newTemplate } from "./overlayDraft";
import { SYSTEM_TEMPLATES } from "./templates";

describe("matchingTemplate", () => {
  it.each(["palette_hue", "palette_chroma"] as const)("detects a changed custom %s", (field) => {
    const recipe = structuredClone(SYSTEM_TEMPLATES.pill);
    recipe.draw.level = "wave";
    const preferences = overlayPreferences({ palette: "custom", palette_hue: 120, palette_chroma: 0.1 });
    preferences.templates = [newTemplate("Custom", recipe, preferences)];
    expect(matchingTemplate(recipe, preferences)?.kind).toBe("mine");
    const changed = { ...preferences, [field]: field === "palette_hue" ? 220 : 0.15 };
    expect(matchingTemplate(recipe, changed)).toBeNull();
  });

  it("ignores inactive custom colour values for preset palettes", () => {
    const recipe = SYSTEM_TEMPLATES.pill;
    const preferences = overlayPreferences({ palette: "graphite" });
    preferences.templates = [newTemplate("Graphite", recipe, preferences)];
    expect(matchingTemplate(recipe, { ...preferences, palette_hue: 240 })?.kind).toBe("mine");
  });
});
