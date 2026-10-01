/** Sending is on only after an explicit yes. A missing key is not consent. */
export function isTelemetryEnabled(value: boolean | undefined): boolean {
  return value === true;
}

/** `true` or `false` in config means the user has already answered. */
export function hasTelemetryDecision(value: boolean | undefined): boolean {
  return typeof value === "boolean";
}

/** The main window asks once those who never reached the introduction's last
 *  step: existing installations and skipped introductions. It waits for a
 *  successful dictation, so the question comes after the product has worked. */
export function shouldOfferTelemetryConsent(config: { telemetry_enabled?: boolean; onboarding_completed?: boolean } | null, transcriptions: number): boolean {
  return !!config && config.onboarding_completed !== false && !hasTelemetryDecision(config.telemetry_enabled) && transcriptions > 0;
}
