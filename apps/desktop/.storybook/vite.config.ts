import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Stories only need Svelte; the Electron build uses electron.vite.config.ts.
export default defineConfig({
  plugins: [svelte({ configFile: resolve(import.meta.dirname, '../svelte.config.mjs') })]
})
