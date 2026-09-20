import { afterEach, describe, expect, it, vi } from "vitest";
import { accentVariables, applyAccent, DEFAULT_ACCENT } from "./accent";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("interface contrast", () => {
  it("survives pending CSS and derives contrast using the latest accent after load", () => {
    const properties = new Map<string, string>();
    let surfaces: string[] = [];
    const document = {
      readyState: "loading",
      documentElement: { style: {
        setProperty: (name: string, value: string) => properties.set(name, value),
        getPropertyValue: (name: string) => properties.get(name) ?? "",
      } },
    };
    const window = new EventTarget();
    const listen = vi.spyOn(window, "addEventListener");
    vi.stubGlobal("document", document);
    vi.stubGlobal("window", window);
    vi.stubGlobal("getComputedStyle", () => ({
      getPropertyValue: (name: string) => surfaces[Number(name.slice(-1))] ?? "",
    }));

    expect(() => applyAccent(DEFAULT_ACCENT)).not.toThrow();
    expect(properties.get("--accent")).toBe(DEFAULT_ACCENT);
    expect(properties.has("--accent-text")).toBe(false);
    applyAccent("#102040");
    expect(listen).toHaveBeenCalledTimes(1);

    surfaces = ["#ebe8e0", "#f5f2ea", "#faf8f2", "#fff", "#f0ece2", "#e6e1d4"];
    document.readyState = "complete";
    window.dispatchEvent(new Event("load"));
    expect(Object.fromEntries(properties)).toEqual(accentVariables("#102040", surfaces));
  });

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
