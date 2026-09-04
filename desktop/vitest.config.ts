/// <reference types="vitest" />
import { defineConfig } from "vite";

// Vitest config — node environment is sufficient for the bridge
// helpers (they don't touch the DOM). The `globals` flag is off so
// the existing frontend code keeps its explicit `vi.fn()` /
// `expect` imports (matching the codebase's style).

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    exclude: ["node_modules", "dist", "src-tauri", "e2e"],
  },
});