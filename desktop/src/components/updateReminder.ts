export const UPDATE_CHECK_INTERVAL = 6 * 60 * 60 * 1000;
export const UPDATE_REMINDER_INTERVAL = 24 * 60 * 60 * 1000;
export const UPDATE_NOTICE_DURATION = 12_000;

export function updateReminderDue(shownAt: number, now: number): boolean {
  return !Number.isFinite(shownAt) || shownAt <= 0 || shownAt > now
    || now - shownAt >= UPDATE_REMINDER_INTERVAL;
}
