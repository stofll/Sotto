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
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "es2022",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
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
