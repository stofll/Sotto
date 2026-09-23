import { describe, expect, it } from "vitest";
import { DEFAULT_RECORDING_LIMIT_MINUTES, recordingLimitMinutes, recordingLimitOptions } from "./recordingLimitSettings";

describe("recording length limit", () => {
  it("defaults when the config has no or an unreadable value", () => {
    for (const value of [undefined, null, -1, 2.5, Number.NaN]) {
      expect(recordingLimitMinutes(value as number)).toBe(DEFAULT_RECORDING_LIMIT_MINUTES);
    }
  });

  it("keeps zero as no limit and caps at a day, like the recorder", () => {
    expect(recordingLimitMinutes(0)).toBe(0);
    expect(recordingLimitMinutes(100_000)).toBe(24 * 60);
  });

  it("lists a hand-edited value in order with no limit last", () => {
    expect(recordingLimitOptions(15)).toEqual([5, 10, 15, 30, 60, 0]);
    expect(recordingLimitOptions(20)).toEqual([5, 10, 15, 20, 30, 60, 0]);
  });
});
