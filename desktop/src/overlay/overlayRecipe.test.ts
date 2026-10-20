import { describe, expect, it } from "vitest";
import { fitProcessDraw, LABEL_MAX_CHARS, normalizeLabel, normalizeRecipe, NOTCH_EAR, processDraws, normalizeTemplates, recipeLayout, shellRadius, shellSize, WINDOW_SIZE, type Recipe } from "./overlayRecipe";

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

describe("fill and texture", () => {
  const styled = (style: unknown) => normalizeRecipe({ shell: "pill", style })!.style;

  it("keeps the new fills and textures and falls back field by field", () => {
    expect(styled({ fill: "smoke", texture: "grain" })).toMatchObject({ fill: "smoke", texture: "grain" });
    expect(styled({ fill: "paper", texture: "chroma", radius: "sharp" })).toMatchObject({ fill: "paper", texture: "chroma", radius: "sharp" });
    expect(styled({ fill: "marble", texture: "sparkle" })).toMatchObject({ fill: "palette", texture: "none" });
  });

  it("draws a recipe saved before textures exactly as before", () => {
    expect(styled({ fill: "light", glow: "2" })).toMatchObject({ fill: "light", texture: "none", glow: "2" });
    expect(styled(undefined).texture).toBe("none");
  });
});

describe("level drawings", () => {
  it("keeps a new drawing where it fits and falls back where it does not", () => {
    const wide = normalizeRecipe({ shell: "pill", slots: { center: "level" }, draw: { level: "tide" } })!;
    expect(wide.draw.level).toBe("tide");
    expect(wide.slots.center).toBe("level");
    // The pixel orb is square or tall: a pill's row has no place for it.
    const misplaced = normalizeRecipe({ shell: "pill", slots: { center: "level" }, draw: { level: "dorb" } })!;
    expect(misplaced.slots.center).toBeNull();
    expect(normalizeRecipe({ shell: "bead", slots: { core: "level" }, draw: { level: "dorb" } })!.slots.core).toBe("level");
    expect(normalizeRecipe({ shell: "pill", slots: { center: "level" }, draw: { level: "tides" } })!.draw.level).toBe("bars");
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
    expect(normalizeRecipe({ shell: "pill" })?.pasted).toEqual({ flash: true, words: true, hold: "normal", exit: "fade" });
    expect(normalizeRecipe({ shell: "pill", pasted: { flash: false, words: false, hold: "long" } })?.pasted)
      .toEqual({ flash: false, words: false, hold: "long", exit: "fade" });
    expect(normalizeRecipe({ shell: "pill", pasted: { flash: "no", hold: "forever" } })?.pasted)
      .toEqual({ flash: true, words: true, hold: "normal", exit: "fade" });
  });

  it("dissolves only when the recipe says so, and falls back to the fade for anything else", () => {
    expect(normalizeRecipe({ shell: "pill", pasted: { exit: "dissolve" } })?.pasted.exit).toBe("dissolve");
    for (const exit of ["melt", 3, null]) expect(normalizeRecipe({ shell: "pill", pasted: { exit } })?.pasted.exit).toBe("fade");
  });
});

describe("processing drawings", () => {
  it("keeps the new drawings where there is room for them", () => {
    for (const draw of ["decode", "waves", "scan", "bar", "halftone"]) {
      expect(normalizeRecipe({ shell: "pill", processing: { draw } })?.processing.draw).toBe(draw);
    }
  });

  it("falls back where a shell is too small for a text drawing", () => {
    // Decoding and the bar need a row: the compact shells and the caption chip give them none.
    for (const shell of ["bead", "stack", "mini", "caps"]) {
      expect(processDraws(shell as never)).not.toContain("decode");
      expect(processDraws(shell as never)).not.toContain("bar");
    }
    expect(normalizeRecipe({ shell: "bead", slots: { core: "level" }, processing: { draw: "decode" } })?.processing.draw).toBe("arc");
    expect(normalizeRecipe({ shell: "island", processing: { draw: "bar" } })?.processing.draw).toBe("dots");
    expect(normalizeRecipe({ shell: "island", processing: { draw: "decode" } })?.processing.draw).toBe("decode");
    // The sign-like drawings go anywhere.
    for (const draw of ["waves", "scan", "halftone"]) expect(processDraws("mini")).toContain(draw);
    expect(fitProcessDraw("mini", "bar", "arc")).toBe("arc");
  });

  it("offers the decoding as a draft drawing for a text row only", () => {
    expect(normalizeRecipe({ shell: "pill", slots: { below: "draft" }, draw: { draft: "decode" } })?.draw.draft).toBe("decode");
    expect(normalizeRecipe({ shell: "pill", slots: { center: "draft" }, draw: { draft: "decode" } })?.slots.center).toBeNull();
  });
});

describe("data elements", () => {
  it("keeps the ring where it has a square or small place and drops it where it has none", () => {
    expect(normalizeRecipe({ shell: "bead", slots: { core: "limit" } })?.slots.core).toBe("limit");
    expect(normalizeRecipe({ shell: "pill", slots: { start: "limit", center: "limit" } })?.slots).toMatchObject({ start: "limit", center: null });
    // The stack's places are tall: neither the ring nor a short figure fits.
    expect(normalizeRecipe({ shell: "stack", slots: { top: "limit", bottom: "words" } })?.slots).toEqual({ top: null, bottom: null });
  });

  it("keeps the figures in a side place and the text elements in a wide one too", () => {
    const row = normalizeRecipe({ shell: "pill", slots: { start: "words", center: "target", end: "pace" } })!;
    expect(row.slots).toMatchObject({ start: "words", center: "target", end: "pace" });
    expect(normalizeRecipe({ shell: "pill", slots: { start: "engine", center: "label" } })?.slots).toMatchObject({ start: "engine", center: "label" });
    // A short figure has no wide place.
    expect(normalizeRecipe({ shell: "pill", slots: { center: "words" } })?.slots.center).toBeNull();
  });

  it("draws a recipe saved before them exactly as before", () => {
    const old = normalizeRecipe({ shell: "pill", slots: { start: "timer", center: "level" }, draw: { timer: "big" } })!;
    expect(old.draw).toMatchObject({ timer: "big", limit: "ring", words: "count", target: "app" });
    expect(old.label).toBeUndefined();
  });
});

describe("timer drawings", () => {
  it("keeps the clocks in a side or wide place and refuses the tall one", () => {
    for (const draw of ["seg", "flip", "led", "glyph", "binary"]) {
      expect(normalizeRecipe({ shell: "pill", slots: { start: "timer" }, draw: { timer: draw } })?.slots.start).toBe("timer");
      expect(normalizeRecipe({ shell: "pill", slots: { center: "timer" }, draw: { timer: draw } })?.slots.center).toBe("timer");
      expect(normalizeRecipe({ shell: "stack", slots: { top: "timer" }, draw: { timer: draw } })?.slots.top).toBeNull();
    }
    expect(normalizeRecipe({ shell: "pill", slots: { start: "timer" }, draw: { timer: "sundial" } })?.draw.timer).toBe("capsule");
  });
});

describe("label", () => {
  it("is trimmed, cut to its length by characters, and absent when empty", () => {
    expect(normalizeRecipe({ shell: "pill", label: "  ✦ работа  " })?.label).toBe("✦ работа");
    expect(Array.from(normalizeRecipe({ shell: "pill", label: "я".repeat(40) })!.label!)).toHaveLength(LABEL_MAX_CHARS);
    for (const label of ["", "   ", 7, null, { text: "x" }]) expect(normalizeRecipe({ shell: "pill", label })).not.toHaveProperty("label");
  });

  it("counts an emoji as one character, as the native side does", () => {
    expect(normalizeLabel("😀".repeat(LABEL_MAX_CHARS + 3))).toBe("😀".repeat(LABEL_MAX_CHARS));
    expect(normalizeLabel(`${"a".repeat(LABEL_MAX_CHARS - 1)} b`)).toBe("a".repeat(LABEL_MAX_CHARS - 1));
  });
});

describe("window, receipt, split and ring", () => {
  const NEW_SHELLS = ["window", "receipt", "split", "ring"] as const;

  it("opens each into the pill row for an error, and keeps its own window otherwise", () => {
    for (const shell of NEW_SHELLS) {
      const recipe = normalizeRecipe({ shell })!;
      expect(recipeLayout(recipe, false, false)).toBe(shell);
      expect(recipeLayout(recipe, true, false)).toBe(shell);
      expect(recipeLayout(recipe, false, true)).toBe("pill");
    }
  });

  it("keeps parts that fit the new regions and drops those that do not", () => {
    expect(normalizeRecipe({ shell: "window", slots: { title: "draft", body: "level", footL: "timer", footR: "mode" }, draw: { level: "tide", mode: "short" } })?.slots)
      .toEqual({ title: "draft", body: "level", footL: "timer", footR: "mode" });
    // The title bar holds one line of text and the body a wide drawing.
    expect(normalizeRecipe({ shell: "window", slots: { title: "timer", body: "draft" } })?.slots).toMatchObject({ title: null, body: null });
    expect(normalizeRecipe({ shell: "receipt", slots: { level: "level", print: "draft", total: "timer" }, draw: { level: "halftone" } })?.slots)
      .toEqual({ level: "level", print: "draft", total: "timer" });
    expect(normalizeRecipe({ shell: "split", slots: { dot: "level", start: "timer", center: "mode" }, draw: { level: "dorb" } })?.slots)
      .toEqual({ dot: "level", start: "timer", center: "mode" });
    expect(normalizeRecipe({ shell: "ring", slots: { hole: "timer", rim: "level" } })?.slots).toEqual({ hole: "timer" });
  });

  it("gives the cancel button its own places and no word where there is no room", () => {
    expect(normalizeRecipe({ shell: "window" })?.cancel.at).toBe("box");
    expect(normalizeRecipe({ shell: "window", cancel: { at: "corner" } })?.cancel.at).toBe("box");
    expect(normalizeRecipe({ shell: "window", cancel: { at: "footL", draw: "text", show: "always" } })?.cancel).toEqual({ at: "footL", draw: "x", show: "always" });
    expect(normalizeRecipe({ shell: "receipt", cancel: { draw: "text" } })?.cancel).toEqual({ at: "corner", draw: "text", show: "hover" });
    expect(normalizeRecipe({ shell: "split", cancel: { at: "end", draw: "text", show: "always" } })?.cancel).toEqual({ at: "dot", draw: "x", show: "hover" });
    expect(normalizeRecipe({ shell: "ring", cancel: { draw: "stop", show: "always" } })?.cancel).toEqual({ at: "hole", draw: "stop", show: "hover" });
  });

  it("keeps the processing drawings that need a row to the shells that have one", () => {
    for (const shell of ["window", "receipt"] as const) expect(processDraws(shell)).toContain("decode");
    expect(processDraws("window")).toContain("bar");
    for (const shell of ["split", "ring"] as const) {
      expect(processDraws(shell)).not.toContain("decode");
      expect(processDraws(shell)).not.toContain("bar");
      expect(processDraws(shell)).toContain("waves");
    }
    expect(normalizeRecipe({ shell: "ring", processing: { draw: "decode" } })?.processing.draw).toBe("arc");
    expect(normalizeRecipe({ shell: "window", processing: { draw: "decode" } })?.processing.draw).toBe("decode");
  });

  it("rounds a window and a receipt only a little, and a split like a pill", () => {
    const round = (shell: string) => shellRadius(normalizeRecipe({ shell })!, "m");
    expect(round("window")).toBeLessThan(8);
    expect(round("receipt")).toBeLessThan(8);
    expect(round("split")).toBe((WINDOW_SIZE.split.m[1] - 8) / 2);
    expect(shellRadius(normalizeRecipe({ shell: "window", style: { radius: "soft" } })!, "m")).toBe(14);
  });
});

describe("notch", () => {
  const notch = normalizeRecipe({ shell: "notch", slots: { earL: "level", earR: "timer" }, draw: { level: "tide" } })!;

  it("keeps a small part in each ear and drops what does not fit", () => {
    expect(notch.slots).toEqual({ earL: "level", earR: "timer" });
    expect(normalizeRecipe({ shell: "notch", slots: { earL: "draft", earR: "limit", centre: "level" } })?.slots).toEqual({ earL: null, earR: "limit" });
  });

  it("keeps its own window, and opens into the pill row for an error", () => {
    expect(recipeLayout(notch, false, false)).toBe("notch");
    expect(recipeLayout(notch, true, false)).toBe("notch");
    expect(recipeLayout(notch, false, true)).toBe("pill");
  });

  it("holds the cancel button in the right ear, on hover, without a word", () => {
    expect(notch.cancel).toEqual({ at: "ear", draw: "x", show: "hover" });
    expect(normalizeRecipe({ shell: "notch", cancel: { at: "end", draw: "text", show: "always" } })?.cancel).toEqual({ at: "ear", draw: "x", show: "hover" });
  });

  it("fills the window without a margin, and follows the native notch when there is one", () => {
    expect(shellSize(notch, "m", "notch", "recording", true)).toEqual([...WINDOW_SIZE.notch.m]);
    expect(shellSize(notch, "m", "notch", "recording", true, { width: 186, height: 38 })).toEqual([186 + 2 * NOTCH_EAR.m, 38]);
    // An error row is an ordinary pill with the margin of any other shell.
    expect(shellSize(notch, "m", "pill", "error", true, { width: 186, height: 38 })).toEqual([WINDOW_SIZE.pill.m[0] - 8, WINDOW_SIZE.pill.m[1] - 8]);
  });
});
