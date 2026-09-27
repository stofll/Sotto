import { describe, expect, it } from "vitest";
import { regionOf } from "../../overlay/overlayRecipe";
import { addPart, changeShell, placeInto, removePart, replacementPatch } from "./recipeEdits";
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

  it("explains a drawing the shell has no place for", () => {
    expect(addPart(pill, "level", "ring")).toMatchObject({ ok: false, reason: "no-fit" });
  });
});

describe("placeInto", () => {
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
