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
    // Три окна приложения — три точки входа. Раньше один bundle обслуживал
    // все три, и накладной pill тянул весь экран настроек: 535 KB на окно,
    // которое показывает строку текста. Разделение по входам оставляет
    // общими только react и общие модули (bridge, i18n, styles.css).
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("index.html", import.meta.url)),
        overlay: fileURLToPath(new URL("overlay.html", import.meta.url)),
        tray: fileURLToPath(new URL("tray.html", import.meta.url)),
      },
    },
  },
}));
