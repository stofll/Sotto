export function sessionIdOf(payload: unknown): number | null {
  if (typeof payload === "number" && Number.isFinite(payload)) return payload;
  if (payload && typeof payload === "object") {
    const value = (payload as { session_id?: unknown }).session_id;
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  }
  return null;
}

export function isCurrentSession(payload: unknown, currentSessionId: number | null): boolean {
  const sessionId = sessionIdOf(payload);
  return sessionId !== null && currentSessionId !== null && sessionId === currentSessionId;
}

export function isCurrentSessionOrUnscoped(payload: unknown, currentSessionId: number | null): boolean {
  return sessionIdOf(payload) === null || isCurrentSession(payload, currentSessionId);
}
