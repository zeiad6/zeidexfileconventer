/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const here = (name: string) => fileURLToPath(new URL(name, import.meta.url));

// Five HTML entries because Tauri opens five windows: the studio shell, the drag wheel, the
// floating orb, the jobs list and the visual editor. Every window is its own React root, so a
// background window never mounts another window's listeners.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: 'ui-dist',
    emptyOutDir: true,
    target: 'chrome110',
    rollupOptions: {
      input: {
        home: here('index.html'),
        wheel: here('wheel.html'),
        shelf: here('shelf.html'),
        jobs: here('jobs.html'),
        editor: here('editor.html'),
      },
    },
  },
  test: {
    include: ['test/**/*.test.{ts,tsx}'],
    environment: 'jsdom',
    setupFiles: './test/setup.ts',
    css: false,
    restoreMocks: true,
  },
});
