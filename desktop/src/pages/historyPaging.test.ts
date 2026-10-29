import { describe, expect, it } from "vitest";
import { pagerItems } from "./HistoryPage";

describe("pagerItems", () => {
  it("shows every page when there are few", () => {
    expect(pagerItems(1, 3)).toEqual([0, 1, 2]);
  });

  it("keeps the ends and the neighbours of the current page", () => {
    expect(pagerItems(9, 19)).toEqual([0, "gap", 8, 9, 10, "gap", 18]);
    expect(pagerItems(0, 19)).toEqual([0, 1, "gap", 18]);
  });

  it("keeps the same compact controls for an unlimited archive", () => {
    expect(pagerItems(50_000, 100_000)).toEqual([0, "gap", 49_999, 50_000, 50_001, "gap", 99_999]);
    expect(pagerItems(99_999, 100_000)).toEqual([0, "gap", 99_998, 99_999]);
  });
});
