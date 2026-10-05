import { describe, expect, it } from "vitest";
import { HISTORY_PAGE_SIZE, historyPage, pagerItems } from "./HistoryPage";

describe("historyPage", () => {
  const items = Array.from({ length: 120 }, (_, index) => index);

  it("slices fifty entries per page", () => {
    expect(historyPage(items, 0).items).toEqual(items.slice(0, HISTORY_PAGE_SIZE));
    expect(historyPage(items, 2)).toMatchObject({ page: 2, pageCount: 3, items: items.slice(100) });
  });

  it("lands on the last page when the list shrank under the current one", () => {
    expect(historyPage(items.slice(0, 60), 2)).toMatchObject({ page: 1, pageCount: 2, items: items.slice(50, 60) });
  });

  it("keeps one empty page for an empty list", () => {
    expect(historyPage([], 3)).toEqual({ items: [], page: 0, pageCount: 1 });
  });
});

describe("pagerItems", () => {
  it("shows every page when there are few", () => {
    expect(pagerItems(1, 3)).toEqual([0, 1, 2]);
  });

  it("keeps the ends and the neighbours of the current page", () => {
    expect(pagerItems(9, 19)).toEqual([0, "gap", 8, 9, 10, "gap", 18]);
    expect(pagerItems(0, 19)).toEqual([0, 1, "gap", 18]);
  });
});
