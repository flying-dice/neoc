/**
 * Web mode: the real desktop renderer in a plain browser, with the in-memory
 * host from `@agentos/sdk/testing` instead of Electron and the main process.
 * Used by Playwright, the design research panel and for quick manual pokes.
 *
 *   bun run dev:web      → http://localhost:6200/web.html?open=calculator
 */
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const here = import.meta.dirname

export default defineConfig({
  root: resolve(here, 'src/renderer'),
  plugins: [svelte({ configFile: resolve(here, 'svelte.config.mjs') })],
  build: {
    target: 'esnext',
    outDir: resolve(here, 'out/web'),
    emptyOutDir: true,
    rollupOptions: { input: resolve(here, 'src/renderer/web.html') }
  },
  server: { port: 6200, strictPort: true },
  preview: { port: 6200, strictPort: true }
})
