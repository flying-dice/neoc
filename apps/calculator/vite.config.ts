import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// `vos()` reads openrpc.json, generates the typed client, the Luau router and the kit's editor types into .app/, puts the
// host's tags (the kit and the bridge) into the page, and builds the app ref into dist/.
export default defineConfig({
  plugins: [svelte({ configFile: false }), vos()]
})
