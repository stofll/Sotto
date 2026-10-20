import { describe, expect, it } from "vitest";
import { countWords, limitProgress, limitSecondsOf, targetLabel, TARGET_MAX_CHARS, usesCloudEngine, wordsPerMinute } from "./recordingFigures";

describe("recording figures", () => {
  it("counts words of a draft whatever the spacing", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("  привет,   мир\nещё ")).toBe(3);
  });

  it("keeps the pace back until the recording is a few seconds old and has words", () => {
    expect(wordsPerMinute(8, 3)).toBeNull();
    expect(wordsPerMinute(0, 30)).toBeNull();
    expect(wordsPerMinute(20, 10)).toBe(120);
    expect(wordsPerMinute(7, 20)).toBe(21);
  });

  it("closes the ring towards the limit and has nothing to close without one", () => {
    expect(limitProgress(150, 300)).toBe(0.5);
    expect(limitProgress(900, 300)).toBe(1);
    expect(limitProgress(-4, 300)).toBe(0);
    expect(limitProgress(150, null)).toBeNull();
    expect(limitProgress(150, 0)).toBeNull();
    expect(limitSecondsOf(0)).toBeNull();
    expect(limitSecondsOf(15)).toBe(900);
  });

  it("calls only the cloud pipeline cloud", () => {
    expect(usesCloudEngine("cloud")).toBe(true);
    expect(usesCloudEngine("hybrid")).toBe(false);
    expect(usesCloudEngine(undefined)).toBe(false);
  });
});

describe("targetLabel", () => {
  it("passes a plain name through and refuses anything that is not text", () => {
    expect(targetLabel("Telegram")).toBe("Telegram");
    expect(targetLabel(null)).toBe("");
    expect(targetLabel(42)).toBe("");
  });

  it("flattens control characters and direction marks and bounds the length", () => {
    expect(targetLabel("  Pages\n‮evil\u0000 ")).toBe("Pages evil");
    expect(Array.from(targetLabel("я".repeat(TARGET_MAX_CHARS + 20)))).toHaveLength(TARGET_MAX_CHARS);
  });
});
