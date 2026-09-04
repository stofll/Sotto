import { describe, expect, it } from "vitest";
import { isTelemetryEnabled } from "./telemetrySettings";

describe("telemetry settings", () => {
  it("defaults missing consent to enabled", () => {
    expect(isTelemetryEnabled(undefined)).toBe(true);
    expect(isTelemetryEnabled(false)).toBe(false);
    expect(isTelemetryEnabled(true)).toBe(true);
  });
});
