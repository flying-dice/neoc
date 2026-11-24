import { resolve } from 'node:path'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

const ui = resolve(import.meta.dirname, '../../../packages/ui/src')

// Stories only need Svelte.
export default defineConfig({
  plugins: [svelte({ configFile: false })],
  resolve: {
    // The kit's source (.storybook/preview.ts defines its elements from it) reads three pure modules of the shell's kit
    // under these names: the same aliases as the kit's own build (packages/kit/vite.config.ts).
    alias: {
      'kit:icons': resolve(ui, 'icons.ts'),
      'kit:contrast': resolve(ui, 'contrast.ts'),
      'kit:keys': resolve(ui, 'keys.ts')
    }
  },
  build: { target: 'esnext' }
})
