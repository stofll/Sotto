import { describe, expect, it } from "vitest";
import { isCurrentSession, isCurrentSessionOrUnscoped, sessionIdOf } from "./sessionEvents";

describe("session event routing", () => {
  it("reads numeric and object session ids", () => {
    expect(sessionIdOf(7)).toBe(7);
    expect(sessionIdOf({ session_id: 8 })).toBe(8);
    expect(sessionIdOf({ message: "engine busy" })).toBeNull();
  });

  it("rejects stale scoped events", () => {
    expect(isCurrentSession({ session_id: 7 }, 8)).toBe(false);
    expect(isCurrentSession({ session_id: 8 }, 8)).toBe(true);
    expect(isCurrentSession({ session_id: 8 }, null)).toBe(false);
  });

  it("allows unscoped startup errors without accepting stale sessions", () => {
    expect(isCurrentSessionOrUnscoped({ message: "engine busy" }, null)).toBe(true);
    expect(isCurrentSessionOrUnscoped({ session_id: 7 }, 8)).toBe(false);
  });
});
