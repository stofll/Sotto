import { describe, expect, it } from "vitest";
import { normalizeRecipe, normalizeTemplates, recipeLayout, shellRadius, shellSize, WINDOW_SIZE, type Recipe } from "./overlayRecipe";

const pill = normalizeRecipe({ shell: "pill", slots: { start: "timer", center: "level", below: "draft" } })!;

describe("normalizeRecipe", () => {
  it("has no recipe without a known shell", () => {
    expect(normalizeRecipe(undefined)).toBeNull();
    expect(normalizeRecipe({ shell: "blob" })).toBeNull();
  });

  it("drops misplaced or repeated elements one by one and keeps the rest", () => {
    const recipe = normalizeRecipe({
      shell: "pill",
      slots: { start: "level", center: "level", end: "draft", below: "draft", ghost: "timer" },
      draw: { level: "beam", timer: "sundial" },
      style: { radius: "wobbly", fill: "black" },
      motion: "teleport",
      matrix: { density: 8, speech: "field" },
    })!;
    // A beam has no place in a pill row, so the level falls back to its default drawing only where it fits.
    expect(recipe.slots).toEqual({ start: null, center: null, end: null, below: "draft" });
    expect(recipe.draw.timer).toBe("capsule");
    expect(recipe.style).toMatchObject({ radius: "round", fill: "black" });
    expect(recipe.motion).toBe("soft");
    expect(recipe.matrix).toEqual({ speech: "field", process: "perimeter", density: 7 });
  });
});

describe("recipeLayout", () => {
  const with_ = (patch: Partial<Recipe>) => ({ ...pill, ...patch });
  it("opens the streaming window only for a draft row", () => {
    expect(recipeLayout(pill, true, false)).toBe("streaming");
    expect(recipeLayout(with_({ slots: { ...pill.slots, below: null } }), true, false)).toBe("pill");
  });
  it("sends round shells to the pill row for text and keeps the card and captions whole", () => {
    const stack = normalizeRecipe({ shell: "stack", slots: { top: "level" } })!;
    expect(recipeLayout(stack, false, false)).toBe("stack");
    expect(recipeLayout(stack, false, true)).toBe("pill");
    expect(recipeLayout(normalizeRecipe({ shell: "card" })!, true, true)).toBe("glow");
    expect(recipeLayout(normalizeRecipe({ shell: "caps" })!, false, true)).toBe("streaming");
  });
});

describe("shellRadius", () => {
  it("keeps one radius per shell and size whatever the state", () => {
    for (const size of ["s", "m", "l"] as const) {
      const radius = shellRadius(pill, size);
      expect(radius).toBe((WINDOW_SIZE.pill[size][1] - 8) / 2);
      // The streaming card is taller, yet its corners stay the pill's.
      const [, height] = shellSize(pill, size, "streaming", "recording", true);
      expect(height).toBeGreaterThan(radius * 2);
    }
    expect(shellRadius({ ...pill, style: { ...pill.style, radius: "sharp" } }, "l")).toBe(3);
  });
});

describe("normalizeTemplates", () => {
  it("keeps valid templates, drops broken ones and repeats", () => {
    const recipe = { shell: "bead", slots: { core: "level" } };
    const templates = normalizeTemplates([
      { id: "a", name: "  Мой  ", recipe, palette: "violet", size: "l" },
      { id: "a", name: "Повтор", recipe },
      { id: "b", name: "", recipe },
      { id: "c", name: "Без рецепта" },
      { id: "d", name: "Цвет", recipe, palette: "neon", palette_hue: 400 },
    ]);
    expect(templates.map((template) => template.id)).toEqual(["a", "d"]);
    expect(templates[0]).toMatchObject({ name: "Мой", palette: "violet", size: "l" });
    expect(templates[1].palette).toBeUndefined();
    expect(templates[1].palette_hue).toBeUndefined();
  });
});

describe("cancel button in a recipe", () => {
  it("gives a recipe saved before it existed the shell's default button", () => {
    expect(normalizeRecipe({ shell: "card" })?.cancel).toEqual({ at: "footR", draw: "x", show: "hover" });
  });

  it("drops a place or a look the shell cannot have", () => {
    expect(normalizeRecipe({ shell: "stack", cancel: { at: "corner", draw: "text", show: "always" } })?.cancel)
      .toEqual({ at: "top", draw: "x", show: "hover" });
  });
});

describe("processing look", () => {
  it("keeps what a recipe saved before the choice showed", () => {
    expect(normalizeRecipe({ shell: "pill", slots: { center: "level" } })?.processing).toEqual({ draw: "dots", edge: true, words: true, speed: "normal" });
    expect(normalizeRecipe({ shell: "bead", slots: { core: "level" }, draw: { level: "ring" } })?.processing.draw).toBe("arc");
    expect(normalizeRecipe({ shell: "pill", slots: { center: "level" }, draw: { level: "matrix" } })?.processing.draw).toBe("matrix");
    // Pixel motion had a cursor and no running light.
    expect(normalizeRecipe({ shell: "pill", slots: { center: "level" }, motion: "pixel" })?.processing).toMatchObject({ draw: "cursor", edge: false });
  });

  it("keeps explicit choices and the old single sign, and drops unknown values one by one", () => {
    expect(normalizeRecipe({ shell: "pill", processing: { draw: "none", edge: false, words: false, speed: "fast" } })?.processing)
      .toEqual({ draw: "none", edge: false, words: false, speed: "fast" });
    expect(normalizeRecipe({ shell: "pill", process: "matrix" })?.processing.draw).toBe("matrix");
    expect(normalizeRecipe({ shell: "bead", processing: { draw: "fireworks", edge: "yes", speed: "warp" } })?.processing)
      .toEqual({ draw: "arc", edge: true, words: true, speed: "normal" });
  });
});

describe("inserted note", () => {
  it("flashes, speaks and stays the usual time unless the recipe says otherwise", () => {
    expect(normalizeRecipe({ shell: "pill" })?.pasted).toEqual({ flash: true, words: true, hold: "normal" });
    expect(normalizeRecipe({ shell: "pill", pasted: { flash: false, words: false, hold: "long" } })?.pasted)
      .toEqual({ flash: false, words: false, hold: "long" });
    expect(normalizeRecipe({ shell: "pill", pasted: { flash: "no", hold: "forever" } })?.pasted)
      .toEqual({ flash: true, words: true, hold: "normal" });
  });
});
