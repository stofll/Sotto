import { confirm as tauriConfirm } from "@tauri-apps/plugin-dialog";

/**
 * A yes/no question about something that cannot be undone.
 *
 * Not `window.confirm`. In this app's WebView2 it returns `true` without ever
 * drawing anything, so every question asked through it was answered «да» by
 * nobody: a profile was deleted the instant its menu item was clicked, and
 * «Очистить всю историю» wiped the history with no dialog in between. The
 * plugin draws a real OS dialog and waits for an answer.
 *
 * A question that could not be put is answered «нет». Every caller is about to
 * destroy something; going ahead because the dialog failed to appear is the
 * same bug in a new place. A dead button is the safer failure.
 */
export async function confirmDestructive(message: string): Promise<boolean> {
  try {
    return await tauriConfirm(message, { kind: "warning" });
  } catch (error) {
    console.error("confirm dialog failed", error);
    return false;
  }
}
