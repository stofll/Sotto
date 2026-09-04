import { describe, expect, it } from "vitest";
import {
  DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES,
  MAX_TELEMETRY_SESSION_TIMEOUT_MINUTES,
  MIN_TELEMETRY_SESSION_TIMEOUT_MINUTES,
  isTelemetryEnabled,
  normalizeTelemetrySessionTimeout,
} from "./telemetrySettings";

describe("telemetry settings", () => {
  it("defaults missing consent to enabled", () => {
    expect(isTelemetryEnabled(undefined)).toBe(true);
    expect(isTelemetryEnabled(false)).toBe(false);
    expect(isTelemetryEnabled(true)).toBe(true);
  });

  it("uses the default for missing or invalid session timeouts", () => {
    expect(normalizeTelemetrySessionTimeout(undefined)).toBe(DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES);
    expect(normalizeTelemetrySessionTimeout("not-a-number")).toBe(DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES);
    expect(normalizeTelemetrySessionTimeout(Number.NaN)).toBe(DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES);
  });

  it("rounds and clamps the session timeout to the contract range", () => {
    expect(normalizeTelemetrySessionTimeout(4)).toBe(MIN_TELEMETRY_SESSION_TIMEOUT_MINUTES);
    expect(normalizeTelemetrySessionTimeout(12.6)).toBe(13);
    expect(normalizeTelemetrySessionTimeout(121)).toBe(MAX_TELEMETRY_SESSION_TIMEOUT_MINUTES);
  });
});
