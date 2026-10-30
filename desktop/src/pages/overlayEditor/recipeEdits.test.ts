import { describe, expect, it } from "vitest";
import { normalizeRecipe, regionOf } from "../../overlay/overlayRecipe";
import { addPart, changeShell, freePartsFor, partsFor, placeInto, removePart, replacementPatch } from "./recipeEdits";
import { SYSTEM_TEMPLATES } from "./templates";

const pill = SYSTEM_TEMPLATES.pill;

describe("addPart", () => {
  it("redraws an element in place when the new drawing fits there", () => {
    const result = addPart(pill, "level", "matrix");
    expect(result.ok && result.recipe.draw.level).toBe("matrix");
    expect(result.ok && regionOf(result.recipe, "level")).toBe("center");
  });

  it("puts a new element in the best free place", () => {
    const result = addPart(pill, "rec", "dot");
    expect(result.ok && regionOf(result.recipe, "rec")).toBe("end");
  });

  it("puts the ring in a pill's centre and in the stack", () => {
    const inPill = addPart(pill, "level", "ring");
    expect(inPill.ok && regionOf(inPill.recipe, "level")).toBe("center");
    const inStack = addPart(SYSTEM_TEMPLATES.stack, "level", "ring");
    expect(inStack.ok && inStack.recipe.draw.level).toBe("ring");
  });

  it("explains a drawing the shell has no place for", () => {
    expect(addPart(pill, "level", "beam")).toMatchObject({ ok: false, reason: "no-fit" });
  });
});

describe("data elements", () => {
  it("land at the side of a row and carry the shell when it changes", () => {
    const withTarget = addPart(pill, "target", "app");
    expect(withTarget.ok && regionOf(withTarget.recipe, "target")).toBe("end");
    if (!withTarget.ok) throw new Error("setup");
    // The row has no free place left, which the editor says instead of displacing a part.
    expect(addPart(withTarget.recipe, "limit", "ring")).toMatchObject({ ok: false, reason: "no-room" });
    // The card's draft takes the body, so the target goes to a side place only when no draft claims it first.
    const moved = changeShell(removePart(withTarget.recipe, "draft"), "card");
    expect(regionOf(moved.recipe, "target")).toBe("footR");
    expect(changeShell(withTarget.recipe, "card").left).toEqual(["target"]);
  });

  it("leaves a shell with no side place for them with a reason", () => {
    expect(addPart(SYSTEM_TEMPLATES.stack, "words", "count")).toMatchObject({ ok: false, reason: "no-fit" });
    expect(changeShell(addPartRecipe(pill, "words", "count"), "stack").left).toContain("words");
  });

  it("keeps a clock drawing of the timer that fits the new place", () => {
    const flip = addPart(pill, "timer", "flip");
    expect(flip.ok && flip.recipe.draw.timer).toBe("flip");
    if (!flip.ok) throw new Error("setup");
    // A flip clock has no tall place: the stack redraws the timer in a drawing that has one.
    const stacked = changeShell(flip.recipe, "stack");
    expect(stacked.switched).toContain("timer");
    expect(["plain", "big"]).toContain(stacked.recipe.draw.timer);
  });
});

function addPartRecipe(recipe: typeof pill, type: Parameters<typeof addPart>[1], draw: string) {
  const result = addPart(recipe, type, draw);
  if (!result.ok) throw new Error("setup");
  return result.recipe;
}

describe("placeInto", () => {
  it("keeps the drawing when it fits the new place", () => {
    const withScope = addPart(pill, "level", "scope");
    if (!withScope.ok) throw new Error("setup");
    const moved = placeInto(withScope.recipe, "level", null, "end");
    expect(moved).toMatchObject({ ok: true, switched: [] });
    expect(moved.ok && moved.recipe.draw.level).toBe("scope");
  });

  it("names an element it had to redraw to fit the new place", () => {
    const bigTimer = placeInto(pill, "timer", "big", "center");
    if (!bigTimer.ok) throw new Error("setup");
    const moved = placeInto(bigTimer.recipe, "timer", null, "end");
    expect(moved).toMatchObject({ ok: true, switched: ["timer"] });
    expect(moved.ok && moved.recipe.draw.timer).toBe("capsule");
  });

  it("swaps two elements when each fits the other's place", () => {
    const withRec = addPart(pill, "rec", "dot");
    if (!withRec.ok) throw new Error("setup");
    const result = placeInto(withRec.recipe, "rec", null, "start");
    if (!result.ok) throw new Error("rejected");
    expect(regionOf(result.recipe, "rec")).toBe("start");
    expect(regionOf(result.recipe, "timer")).toBe("end");
  });

  it("refuses a drawing that does not fit the region", () => {
    expect(placeInto(pill, "timer", "big", "start")).toMatchObject({ ok: false, reason: "no-fit", region: "start" });
  });
});

describe("changeShell", () => {
  it("carries elements over, redrawing and leaving out what has no place", () => {
    const result = changeShell(pill, "bead");
    expect(regionOf(result.recipe, "level")).toBe("core");
    expect(result.recipe.draw.level).not.toBe("bars");
    expect(result.switched).toContain("level");
    expect(result.left).toEqual(expect.arrayContaining(["draft", "timer"]));
  });

  it("puts the draft back under the row when returning to a pill", () => {
    const back = changeShell(changeShell(pill, "caps").recipe, "pill");
    expect(back.recipe.slots.below).toBe("draft");
  });
});

it("removes an element and leaves its region empty", () => {
  expect(regionOf(removePart(pill, "timer"), "timer")).toBeNull();
});

describe("replacementPatch", () => {
  it("clears keys the new value does not have, at every level", () => {
    const before = { shell: "pill", slots: { start: "timer", below: "draft" }, draw: { level: "bars" } };
    const after = { shell: "bead", slots: { core: "level" }, draw: { level: "ring" } };
    expect(replacementPatch(before, after)).toEqual({ shell: "bead", slots: { start: null, below: null, core: "level" }, draw: { level: "ring" } });
    expect(replacementPatch(null, after)).toBe(after);
  });
});

describe("the cancel button", () => {
  it("keeps a place and a look the new shell has, and falls back where it has none", () => {
    const text = { ...pill, cancel: { at: "start", draw: "text", show: "always" } } as typeof pill;
    expect(changeShell(text, "caps").recipe.cancel).toEqual({ at: "start", draw: "text", show: "always" });
    // The bead's centre is shared with the level: no word, and only on hover.
    expect(changeShell(text, "bead").recipe.cancel).toEqual({ at: "center", draw: "x", show: "hover" });
  });
});

describe("partsFor", () => {
  it("offers only the drawings that fit an empty region", () => {
    const parts = partsFor(removePart(SYSTEM_TEMPLATES.stack, "timer"), "bottom");
    expect(parts.find((part) => part.type === "timer")?.draws).toEqual(["plain", "big"]);
    expect(parts.find((part) => part.type === "mode")?.draws).toEqual(["short"]);
    expect(parts.some((part) => part.type === "draft")).toBe(false);
  });

  it("offers the text only for the line under a pill", () => {
    expect(partsFor(pill, "below")).toEqual([{ type: "draft", draws: ["tail", "plain", "decode"] }]);
  });

  it("offers nothing for a region the shell does not have", () => {
    expect(partsFor(pill, "core")).toEqual([]);
  });
});

describe("freePartsFor", () => {
  it("leaves out the parts already in the shell", () => {
    // The pill template has the timer at the start and the level in the centre.
    expect(freePartsFor(pill, "end").map((part) => part.type)).toEqual(["rec", "mode", "limit", "words", "pace", "engine", "label", "target"]);
    expect(freePartsFor(pill, "end").find((part) => part.type === "timer")).toBeUndefined();
  });

  it("offers the ring only where it has a square or small place, and the bead's core takes nothing but the ring and the level", () => {
    expect(partsFor(pill, "center").map((part) => part.type)).not.toContain("limit");
    expect(partsFor(SYSTEM_TEMPLATES.bead, "core").map((part) => part.type)).toEqual(["level", "rec", "limit"]);
    expect(partsFor(SYSTEM_TEMPLATES.stack, "top").map((part) => part.type)).not.toContain("limit");
  });
});

describe("changeShell and the processing drawing", () => {
  it("keeps a text drawing in a shell with a row and swaps it for the shell's default in a small one", () => {
    const decoding = { ...pill, processing: { ...pill.processing, draw: "decode" as const } };
    expect(changeShell(decoding, "island").recipe.processing.draw).toBe("decode");
    expect(changeShell(decoding, "mini").recipe.processing.draw).toBe("arc");
    const waves = { ...pill, processing: { ...pill.processing, draw: "waves" as const } };
    expect(changeShell(waves, "bead").recipe.processing.draw).toBe("waves");
  });
});

describe("the split, ring and notch shells", () => {
  it("each system template is a recipe that survives normalisation unchanged", () => {
    for (const shell of ["split", "ring", "notch"] as const) {
      const template = SYSTEM_TEMPLATES[shell];
      expect(template.shell).toBe(shell);
      expect(normalizeRecipe(template)).toEqual(template);
      expect(Object.values(template.slots).some(Boolean)).toBe(true);
    }
  });

  it("puts a new part where the shell was designed to hold it", () => {
    const split = addPart(removePart(SYSTEM_TEMPLATES.split, "timer"), "limit", "ring");
    expect(split.ok && regionOf(split.recipe, "limit")).toBe("start");
  });

  it("puts the level in the left ear and the timer in the right when a pill moves to the notch", () => {
    const notch = changeShell(pill, "notch");
    expect(regionOf(notch.recipe, "level")).toBe("earL");
    expect(regionOf(notch.recipe, "timer")).toBe("earR");
    expect(notch.left).toContain("draft");
  });

  it("leaves out what the ring's hole cannot hold when a shell with a draft moves to it", () => {
    const result = changeShell(pill, "ring");
    expect(result.left).toContain("draft");
    expect(Object.keys(result.recipe.slots)).toEqual(["hole"]);
  });

  it("falls back to a sign when a decoding processing moves to a shell without a row", () => {
    const decoding = { ...SYSTEM_TEMPLATES.plank, processing: { ...SYSTEM_TEMPLATES.plank.processing, draw: "decode" as const } };
    // Pixel motion shows a block cursor where the other motions show an arc.
    expect(changeShell(decoding, "split").recipe.processing.draw).toBe("cursor");
    expect(changeShell(decoding, "card").recipe.processing.draw).toBe("decode");
  });
});
