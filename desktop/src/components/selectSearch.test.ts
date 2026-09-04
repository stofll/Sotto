import { describe, expect, it } from "vitest";
import { optionMatches } from "./selectSearch";

describe("select search", () => {
  const german = { label: "Немецкий", meta: "DE" };

  it("finds a language by its name whatever the case", () => {
    expect(optionMatches(german, "нем")).toBe(true);
    expect(optionMatches(german, "НЕМЕЦ")).toBe(true);
  });

  it("finds a language by its code", () => {
    // Языков под сотню, и код часто короче и вернее названия.
    expect(optionMatches(german, "de")).toBe(true);
  });

  it("ignores the spaces around what was typed", () => {
    expect(optionMatches(german, "  нем  ")).toBe(true);
    // Пустой запрос — это не «ничего не подходит», а «ещё не искали».
    expect(optionMatches(german, "   ")).toBe(true);
  });

  it("says no when nothing matches", () => {
    expect(optionMatches(german, "рус")).toBe(false);
  });
});
