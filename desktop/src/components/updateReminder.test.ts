import { describe, expect, it } from "vitest";
import { UPDATE_REMINDER_INTERVAL, updateReminderDue } from "./updateReminder";

describe("update reminder cadence", () => {
  const now = 1_800_000_000_000;

  it("shows the first reminder and recovers from malformed or future storage", () => {
    for (const timestamp of [0, -1, NaN, Infinity, now + 1]) {
      expect(updateReminderDue(timestamp, now)).toBe(true);
    }
  });

  it("limits reminders to once per 24 hours across restarts", () => {
    const shownAt = now - UPDATE_REMINDER_INTERVAL;
    expect(updateReminderDue(shownAt, now - 1)).toBe(false);
    expect(updateReminderDue(shownAt, now)).toBe(true);
    expect(updateReminderDue(now, now)).toBe(false);
  });
});
