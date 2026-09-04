function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function waitForReady(timeoutMs = 10_000): Promise<void> {
  if (!hasTauri()) {
    // HTTP mode: poll /ready endpoint
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        const resp = await fetch("http://127.0.0.1:9137/ready");
        if (resp.ok) return;
      } catch { /* bridge not up yet */ }
      await new Promise((r) => setTimeout(r, 200));
    }
    throw new Error("Sidecar HTTP bridge failed to become ready within timeout");
  }

  // Tauri mode: the Rust core is the same process as the
  // renderer — by the time the JS bundle runs the native
  // handlers are already wired. No readiness handshake is
  // required. The HTTP bridge fallback below is preserved
  // for the dev-only Python test client.
  return new Promise<void>(resolve => { resolve(); });
}
