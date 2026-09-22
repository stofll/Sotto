import { describe, expect, it } from "vitest";
import { newestSetupStatus, type SetupStatus } from "./installer";

const state = (revision: number, phase: SetupStatus["phase"]): SetupStatus =>
  ({ revision, phase, preview: false, version: "1.0.0", error: null });

describe("installer state delivery", () => {
  it("does not rewind a completed installation when its initial snapshot arrives late", () => {
    const completed = state(3, "complete");
    expect(newestSetupStatus(completed, state(1, "preparing"))).toBe(completed);
  });
  it("accepts an error and the subsequent retry", () => {
    const failed = { ...state(3, "failed"), error: "install_failed" };
    const retry = state(4, "preparing");
    expect(newestSetupStatus(state(2, "installing"), failed)).toBe(failed);
    expect(newestSetupStatus(failed, retry)).toBe(retry);
  });
});
