import { defineConfig } from 'electron-vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

/** Workspace packages ship TypeScript source, so they are bundled, not externalized. */
const workspacePackages = ['@agentos/sdk', '@agentos/ui', '@agentos/app-calculator']

export default defineConfig({
  main: {
    build: { externalizeDeps: { exclude: workspacePackages } }
  },
  // Sandboxed preloads can't require npm packages at runtime: bundle everything.
  preload: {
    build: { externalizeDeps: false }
  },
  renderer: {
    plugins: [svelte()]
  }
})
