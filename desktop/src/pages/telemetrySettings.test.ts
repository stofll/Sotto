import { describe, expect, it } from "vitest";
import { hasTelemetryDecision, isTelemetryEnabled, shouldOfferTelemetryConsent } from "./telemetrySettings";

describe("telemetry settings", () => {
  it("sends only after an explicit yes", () => {
    expect(isTelemetryEnabled(undefined)).toBe(false);
    expect(isTelemetryEnabled(false)).toBe(false);
    expect(isTelemetryEnabled(true)).toBe(true);
  });

  it("treats only a stored boolean as an answer", () => {
    expect(hasTelemetryDecision(undefined)).toBe(false);
    expect(hasTelemetryDecision(false)).toBe(true);
    expect(hasTelemetryDecision(true)).toBe(true);
  });

  it("asks once after a dictation when no answer is stored", () => {
    expect(shouldOfferTelemetryConsent({}, 1)).toBe(true);
    expect(shouldOfferTelemetryConsent({ onboarding_completed: true }, 3)).toBe(true);
    expect(shouldOfferTelemetryConsent({}, 0)).toBe(false);
    expect(shouldOfferTelemetryConsent({ telemetry_enabled: false }, 5)).toBe(false);
    expect(shouldOfferTelemetryConsent({ telemetry_enabled: true }, 5)).toBe(false);
    // The introduction's last step asks the same question itself.
    expect(shouldOfferTelemetryConsent({ onboarding_completed: false }, 5)).toBe(false);
    expect(shouldOfferTelemetryConsent(null, 5)).toBe(false);
  });
});
