import type { RuntimeStatusResult } from "../bridge/types";

/**
 * Runtime is authoritative for what can be claimed as loaded. The persisted
 * config may intentionally keep the previous selection after a failed switch.
 */
export function actualModelLabel(
  runtime: Pick<RuntimeStatusResult, "loaded_model" | "active_model"> | null | undefined,
  fallback: string,
): string {
  // New runtime snapshots always include `active_model`, including null in
  // an invalid/unconfigured cloud setup. Do not fall back to a stale local
  // engine in that case. Older snapshots without the field remain supported.
  const hasEffectiveRoute = !!runtime && "active_model" in runtime;
  const loaded = (hasEffectiveRoute ? runtime?.active_model : runtime?.loaded_model)?.trim();
  return loaded || fallback;
}

export function actualEngineLabel(
  runtime: Pick<RuntimeStatusResult, "engine" | "active_engine"> | null | undefined,
): string {
  const hasEffectiveRoute = !!runtime && "active_engine" in runtime;
  return (hasEffectiveRoute ? runtime?.active_engine : runtime?.engine)?.trim() || "STT";
}

export function actualDeviceLabel(
  runtime: Pick<RuntimeStatusResult, "device" | "active_device"> | null | undefined,
): string {
  const hasEffectiveRoute = !!runtime && "active_device" in runtime;
  return (hasEffectiveRoute ? runtime?.active_device : runtime?.device)?.trim() || "—";
}
