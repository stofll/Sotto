import { describe, expect, it } from "vitest";
import { wordDiff, type DiffSegment } from "./textDiff";

/** What the block actually renders, as `text` per change kind. */
function changed(before: string, after: string): DiffSegment[] {
  return wordDiff(before, after).filter((segment) => segment.change !== "keep");
}

/** Round-tripping is the invariant that matters: dropping or duplicating a
 *  character while narrowing the highlight would silently misreport the text. */
function reconstruct(segments: DiffSegment[], side: "before" | "after"): string {
  const skip = side === "before" ? "add" : "remove";
  return segments.filter((s) => s.change !== skip).map((s) => s.text).join("");
}

describe("wordDiff", () => {
  it("marks nothing when the texts match", () => {
    const segments = wordDiff("привет как дела", "привет как дела");
    expect(segments.every((s) => s.change === "keep")).toBe(true);
  });

  it("highlights only the added comma, not the word carrying it", () => {
    // The case this refinement exists for: before the character pass the whole
    // word read as deleted-and-reinserted, so two identical-looking words sat
    // side by side and the eye had to hunt for the comma.
    expect(changed("Спичу текст делать конечно", "Спичу текст делать, конечно")).toEqual([
      { text: ",", change: "add" },
    ]);
  });

  it("highlights only the removed comma", () => {
    expect(changed("текст, конечно", "текст конечно")).toEqual([
      { text: ",", change: "remove" },
    ]);
  });

  it("narrows a case change to the letter that changed", () => {
    expect(changed("то есть", "То есть")).toEqual([
      { text: "т", change: "remove" },
      { text: "Т", change: "add" },
    ]);
  });

  it("narrows an added prefix", () => {
    expect(changed("довольно легко", "довольно нелегко")).toEqual([
      { text: "не", change: "add" },
    ]);
  });

  it("narrows a changed ending", () => {
    expect(changed("они заебывать уже", "они заебывают уже")).toEqual([
      { text: "ть", change: "remove" },
      { text: "ют", change: "add" },
    ]);
  });

  it("keeps unrelated words whole instead of splitting them into letters", () => {
    // «материал» and «модель» share only a leading «м». Peeling that off and
    // highlighting the rest character by character is noisier than replacing
    // the word, so the refinement declines below MIN_COMMON_SHARE.
    expect(changed("этот материал", "этот модель")).toEqual([
      { text: "материал", change: "remove" },
      { text: "модель", change: "add" },
    ]);
  });

  it("narrows a comma inserted between two words", () => {
    expect(changed("моделька которая всё", "моделька, которая всё")).toEqual([
      { text: ",", change: "add" },
    ]);
  });

  it("still reports pure insertions and deletions", () => {
    expect(changed("привет дела", "привет как дела")).toEqual([
      { text: "как ", change: "add" },
    ]);
    expect(changed("привет как дела", "привет дела")).toEqual([
      { text: "как ", change: "remove" },
    ]);
  });

  it("handles a paragraph break replacing a space", () => {
    const segments = wordDiff("первая мысль. вторая мысль.", "первая мысль.\n\nвторая мысль.");
    expect(reconstruct(segments, "before")).toBe("первая мысль. вторая мысль.");
    expect(reconstruct(segments, "after")).toBe("первая мысль.\n\nвторая мысль.");
  });

  it("reconstructs both sides exactly", () => {
    const before = "Спичу текст делать конечно пиздец как не просто. то есть если делать просто транскрибацию";
    const after = "Спичу текст делать, конечно, пиздец, как не просто. То есть, если делать просто транскрибацию,";
    const segments = wordDiff(before, after);
    expect(reconstruct(segments, "before")).toBe(before);
    expect(reconstruct(segments, "after")).toBe(after);
  });

  it("survives empty input on either side", () => {
    expect(reconstruct(wordDiff("", "привет"), "after")).toBe("привет");
    expect(reconstruct(wordDiff("привет", ""), "before")).toBe("привет");
    expect(wordDiff("", "")).toEqual([]);
  });

  it("does not split a surrogate pair while narrowing", () => {
    // Slicing by UTF-16 unit would cut an emoji in half and render U+FFFD.
    const segments = wordDiff("готово 🙂", "готово 🙃");
    expect(reconstruct(segments, "before")).toBe("готово 🙂");
    expect(reconstruct(segments, "after")).toBe("готово 🙃");
    expect(segments.some((s) => s.text.includes("�"))).toBe(false);
  });
});
