import { describe, expect, it } from "vitest";
import { accentVariables, DEFAULT_ACCENT } from "./accent";

describe("interface contrast", () => {
  it.each([DEFAULT_ACCENT, "#102040", "#f2e14a"])("handles minified light surfaces with accent %s", (color) => {
    const surfaces = ["#ebe8e0", "#f5f2ea", "#faf8f2", "#ffffff", "#f0ece2", "#e6e1d4"];
    const minified = surfaces.map((surface) => surface === "#ffffff" ? "#fff" : surface);
    expect(accentVariables(color, minified)).toEqual(accentVariables(color, surfaces));
  });

  it("treats short hex and computed RGB as the same surface", () => {
    const expected = accentVariables(DEFAULT_ACCENT, ["#aabbcc"]);
    expect(accentVariables(DEFAULT_ACCENT, ["#abc"])).toEqual(expected);
    expect(accentVariables(DEFAULT_ACCENT, ["rgb(170, 187, 204)"])).toEqual(expected);
  });
});
