import { describe, expect, it } from "vitest";
import { menuPlacement } from "./anchoredMenu";

describe("menu placement", () => {
  it("shows the whole list when it fits below", () => {
    expect(menuPlacement(400, 280, 300)).toEqual({ height: 300, scrolls: false });
  });

  it("scrolls a list cut short by the window edge instead of moving it", () => {
    expect(menuPlacement(220, 280, 300)).toEqual({ height: 220, scrolls: true });
  });

  it("keeps a usable height right at the bottom of the window", () => {
    expect(menuPlacement(40, 280, 300)).toEqual({ height: 140, scrolls: true });
  });

  it("scrolls a list longer than the height cap", () => {
    expect(menuPlacement(800, 2000, 260)).toEqual({ height: 260, scrolls: true });
  });
});
