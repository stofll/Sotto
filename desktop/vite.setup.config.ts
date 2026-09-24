import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL("setup", import.meta.url)),
  // A separate dependency cache: sharing node_modules/.vite with a running
  // application dev server makes its lazily loaded dependencies fail with 504.
  // Browser tests give each dev server its own cache the same way.
  cacheDir:
    process.env.SOTTO_VITE_CACHE_DIR ??
    fileURLToPath(new URL("node_modules/.vite-setup", import.meta.url)),
  publicDir: false,
  plugins: [react()],
  server: { host: "127.0.0.1", port: 1421, strictPort: true },
  build: { target: "chrome105", outDir: "dist", emptyOutDir: true },
});
