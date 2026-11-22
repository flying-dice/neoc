/** Builds the client into one classic script, `dist/client.js`, that defines `vosApp.mount` (see apps/README.md). */
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

export default defineConfig({
  // `css: 'injected'` keeps the component styles in the script: an app ships one file
  plugins: [svelte({ configFile: false, compilerOptions: { css: 'injected' } })],
  build: {
    lib: { entry: 'src/main.ts', name: 'vosApp', formats: ['iife'], fileName: () => 'client.js' },
    outDir: 'dist',
    emptyOutDir: true
  }
})
