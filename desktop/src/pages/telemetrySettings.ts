/** Missing telemetry consent is intentionally treated as enabled for v1. */
export function isTelemetryEnabled(value: boolean | undefined): boolean {
  return value ?? true;
}
