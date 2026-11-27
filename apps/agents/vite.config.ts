import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// `vos()` reads package.json (what the app is) and openrpc.json (its API), writes their types into types/ (nothing it
// generates runs), puts the host's tags (the kit and the bridge) into the page, and builds the app ref into dist/.
export default defineConfig({
  plugins: [svelte({ configFile: false }), vos()]
})
