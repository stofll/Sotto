import { describe, expect, it } from "vitest";
import { menuPlacement } from "./anchoredMenu";

describe("menu placement", () => {
  it("shows the whole list when it fits below", () => {
    expect(menuPlacement(400, 300, 280, 300)).toEqual({ opensAbove: false, height: 300, scrolls: false });
  });

  it("scrolls a list cut short by the window edge instead of moving it", () => {
    expect(menuPlacement(220, 100, 280, 300)).toEqual({ opensAbove: false, height: 220, scrolls: true });
  });

  it("opens above the button at the bottom of the window", () => {
    expect(menuPlacement(40, 320, 280, 300)).toEqual({ opensAbove: true, height: 300, scrolls: false });
  });

  it("scrolls a list longer than the height cap", () => {
    expect(menuPlacement(800, 0, 2000, 260)).toEqual({ opensAbove: false, height: 260, scrolls: true });
  });
});
