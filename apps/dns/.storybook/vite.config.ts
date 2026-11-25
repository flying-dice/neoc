import { resolve } from 'node:path'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

const ui = resolve(import.meta.dirname, '../../../packages/ui/src')

// Stories only need Svelte. The app's build (../vite.config.ts) is not used: its `vos()` plugin generates the backend's
// router, checks the Luau and writes the app ref into dist/, none of which a Storybook should do, and it resolves
// `@dns/rpc` to the client of a host that is not there.
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
