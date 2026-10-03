import { invoke } from "./invoke";
import type { UpdateInfo } from "./types";

const pending = new Map<boolean, Promise<UpdateInfo>>();

/** Share overlapping checks, including StrictMode's effect replay. */
export function checkUpdate(receiveBeta: boolean): Promise<UpdateInfo> {
  const current = pending.get(receiveBeta);
  if (current) return current;
  const request = invoke<UpdateInfo>("check_update").finally(() => {
    pending.delete(receiveBeta);
  });
  pending.set(receiveBeta, request);
  return request;
}
