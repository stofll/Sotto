import { defineConfig } from '../../desktop/node_modules/vite/dist/node/index.js';
import react from '../../desktop/node_modules/@vitejs/plugin-react/dist/index.js';
import { fileURLToPath } from 'node:url';

const desktop = fileURLToPath(new URL('../../desktop/', import.meta.url));
export default defineConfig({
  root: fileURLToPath(new URL('./', import.meta.url)),
  plugins: [react()],
  resolve: {
    alias: {
      react: `${desktop}node_modules/react`,
      'react-dom': `${desktop}node_modules/react-dom`,
      '@tauri-apps/api': `${desktop}node_modules/@tauri-apps/api`,
    },
  },
  build: { outDir: fileURLToPath(new URL('../../tools/speech-eval/annotation-site/dist/', import.meta.url)), emptyOutDir: true },
});
