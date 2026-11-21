import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Stories only need Svelte (the app build is ../vite.config.ts); esnext matches that build's target.
export default defineConfig({
  plugins: [svelte({ configFile: resolve(import.meta.dirname, '../svelte.config.mjs') })],
  build: { target: 'esnext' },
  optimizeDeps: { esbuildOptions: { target: 'esnext' } }
})
