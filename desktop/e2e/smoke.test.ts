/**
 * E2E smoke test for Whisper Desktop (macOS).
 *
 * Prerequisites:
 *   - The debug binary built with `pnpm tauri build --debug --no-bundle`
 *   - The app binary is running (launched by wdio.conf.js beforeSession)
 *   - `tauri-plugin-webdriver` is registered in `lib.rs` under `#[cfg(debug_assertions)]`
 *   - WebDriverIO v9+ and its Tauri support are installed (see wdio.conf.js)
 *
 * This test connects to the embedded WebDriver server (port 4445),
 * navigates to the app frontend, invokes the native `start_recording`
 * command, and verifies the `whisper-done` event carries a non-empty
 * transcription result.
 *
 * Run with: npx wdio run e2e/wdio.conf.js
 *
 * For deterministic CI runs, build with `--features test-commands` and
 * use `start_recording_test` to inject known audio instead of relying
 * on the physical microphone.
 */

import { browser, expect } from "@wdio/globals";

describe("Whisper Desktop E2E", () => {
  it("app loads and renders the root element", async () => {
    await browser.url("tauri://localhost");
    const root = await $("#root");
    await root.waitForDisplayed({ timeout: 15_000 });
    expect(await root.isDisplayed()).toBe(true);
  });

  it("start_recording returns a positive session ID", async () => {
    // Execute a Tauri IPC command from inside the webview context.
    // The executeAsync endpoint serialises the callback result back.
    const sessionId: unknown = await browser.executeAsync(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (done: (result: any) => void) => {
        import("@tauri-apps/api/core")
          .then(({ invoke }) => invoke<number>("start_recording"))
          .then(done)
          .catch((err: Error) => done({ error: err.message }));
      },
    );

    // Guard against the error shape returned on rejection
    if (typeof sessionId === "object" && sessionId !== null) {
      const obj = sessionId as Record<string, unknown>;
      expect(obj).not.toHaveProperty("error");
    }
    expect(sessionId).toBeGreaterThan(0);
  });

  it("receives whisper-done with non-empty text", async () => {
    // Register a one-shot listener for the whisper-done event.
    // Times out after 30 s to avoid hanging the suite on mic silence.
    const payload: unknown = await browser.executeAsync(
      (done: (result: unknown) => void) => {
        const timeoutId = setTimeout(() => {
          done({ error: "timeout waiting for whisper-done (30 s)" });
        }, 30_000);

        import("@tauri-apps/api/event")
          .then(({ listen }) =>
            listen<{ text: string }>("whisper-done", (event) => {
              clearTimeout(timeoutId);
              // Return only the serialisable payload, not the unlisten fn
              done(event.payload);
            }),
          )
          .catch((err: Error) => {
            clearTimeout(timeoutId);
            done({ error: err.message });
          });
      },
    );

    expect(payload).not.toBeNull();
    const obj = payload as Record<string, unknown>;
    expect(obj).not.toHaveProperty("error");
    expect(obj).toHaveProperty("text");
    expect(typeof obj.text).toBe("string");
    expect((obj.text as string).length).toBeGreaterThan(0);
  });
});
