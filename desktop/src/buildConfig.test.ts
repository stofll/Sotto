// Guards on the Vite configuration that no build output reveals: a variable
// exposed to the client appears in a bundle only once some code reads
// `import.meta.env`, and an unset debug flag builds without complaint.

import { describe, expect, it } from "vitest";
import viteConfig from "../vite.config.ts?raw";

describe("Vite build configuration", () => {
  it("exposes only VITE_ and TAURI_ENV_ variables to the client", () => {
    const declaration = viteConfig.match(/envPrefix:\s*\[([^\]]*)\]/);
    expect(declaration, "envPrefix not found").not.toBeNull();
    const prefixes = [...declaration![1].matchAll(/["'`]([^"'`]+)["'`]/g)].map((m) => m[1]);
    expect(prefixes.length).toBeGreaterThan(0);
    // "TAURI_" alone would include TAURI_SIGNING_PRIVATE_KEY and its password.
    for (const prefix of prefixes) {
      expect(prefix === "VITE_" || prefix.startsWith("TAURI_ENV_"), prefix).toBe(true);
    }
  });

  it("reads the Tauri 2 build variables", () => {
    // Tauri 1 names, which the Tauri 2 CLI no longer sets.
    expect(viteConfig).not.toMatch(/\bTAURI_(PLATFORM|DEBUG|ARCH|FAMILY)\b/);
    expect(viteConfig).toMatch(/TAURI_ENV_DEBUG === "true"/);
  });
});
