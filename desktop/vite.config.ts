import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  // Only the variables the Tauri CLI sets for the frontend build. A bare
  // "TAURI_" prefix would also expose TAURI_SIGNING_PRIVATE_KEY and its
  // password, which the release build has in the same environment.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "es2022",
    minify: process.env.TAURI_ENV_DEBUG === "true" ? false : "esbuild",
    sourcemap: process.env.TAURI_ENV_DEBUG === "true",
    modulePreload: {
      // WebKit retains a failed modulepreload across reloads. Let import()
      // fetch the lazy entry itself so Reload can retry; still preload its
      // dependencies and every HTML entry's startup graph.
      resolveDependencies: (file, dependencies, context) => context.hostType === "js"
        ? dependencies.filter((dependency) => dependency !== file)
        : dependencies,
    },
    // Three application windows — three entry points. One bundle used to serve
    // all three, and the overlay pill dragged in the entire settings screen:
    // 535 KB for a window that shows a single line of text. Splitting by entry
    // leaves only react and the shared modules (bridge, i18n, styles.css) common.
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("index.html", import.meta.url)),
        overlay: fileURLToPath(new URL("overlay.html", import.meta.url)),
        tray: fileURLToPath(new URL("tray.html", import.meta.url)),
      },
    },
  },
}));
