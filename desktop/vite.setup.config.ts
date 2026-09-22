import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL("setup", import.meta.url)),
  publicDir: false,
  plugins: [react()],
  server: { host: "127.0.0.1", port: 1421, strictPort: true },
  build: { target: "chrome105", outDir: "dist", emptyOutDir: true },
});

\n