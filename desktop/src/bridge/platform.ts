// Platform detection for the frontend.
//
// One source of truth: `std::env::consts::OS` as reported by
// `get_runtime_status`, which every window already fetches on mount. The two
// alternatives were rejected for reasons worth keeping: `@tauri-apps/plugin-os`
// answers asynchronously and would add a second wait before the first render,
// and a build-time flag would split the bundle per platform for a value the
// application can read at runtime.
//
// The tray is the only caller today. Its popup window is Windows-only —
// `show_tray_popup` and `hide_tray_popup` are registered under
// `#[cfg(windows)]` — so the tray asks here instead of comparing the OS
// itself, and any future platform-gated window has the same one place to use.
//
// The answer below is about a reported OS only. What an *unreported* one means
// is the caller's decision, because it depends on what the caller loses by
// guessing wrong in either direction: the status arrives one tick after mount,
// and until then a window knows nothing about where it is running. The tray
// treats that tick as Windows and says why.

/** Whether `get_runtime_status.os` names Windows. `undefined` — not reported
 *  yet — is not Windows here; see the note above before relying on that. */
export function isWindowsOs(os: string | null | undefined): boolean {
  return os === "windows";
}
