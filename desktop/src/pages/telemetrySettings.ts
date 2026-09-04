export const DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES = 30;
export const MIN_TELEMETRY_SESSION_TIMEOUT_MINUTES = 5;
export const MAX_TELEMETRY_SESSION_TIMEOUT_MINUTES = 120;

/**
 * The Rust config validator is authoritative, but keeping the same guard in
 * the UI prevents an invalid value from being sent during an intermediate
 * save. Missing or corrupt values use the documented default.
 */
export function normalizeTelemetrySessionTimeout(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return DEFAULT_TELEMETRY_SESSION_TIMEOUT_MINUTES;
  return Math.max(
    MIN_TELEMETRY_SESSION_TIMEOUT_MINUTES,
    Math.min(MAX_TELEMETRY_SESSION_TIMEOUT_MINUTES, Math.round(parsed)),
  );
}

/** Missing telemetry consent is intentionally treated as enabled for v1. */
export function isTelemetryEnabled(value: boolean | undefined): boolean {
  return value ?? true;
}
