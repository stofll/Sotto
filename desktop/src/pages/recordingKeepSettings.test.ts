import { describe, expect, it } from "vitest";
import { recordingKeepCount, recordingKeepOptions } from "./recordingKeepSettings";

describe("recording keep count", () => {
  it("reads what the pruning reads", () => {
    expect(recordingKeepCount(undefined)).toBe(50);
    expect(recordingKeepCount(200)).toBe(200);
    // 0 is "keep everything", not "keep nothing".
    expect(recordingKeepCount(0)).toBe(0);
    expect(recordingKeepCount(-1)).toBe(50);
    expect(recordingKeepCount(2.5)).toBe(50);
  });

  it("shows a hand-edited value instead of rewriting it", () => {
    expect(recordingKeepOptions(50)).toEqual([50, 200, 1000, 0]);
    expect(recordingKeepOptions(75)).toEqual([50, 75, 200, 1000, 0]);
  });
});
