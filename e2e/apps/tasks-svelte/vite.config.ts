import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// `vos()` reads rpc.openrpc.json, generates the typed client and the Luau router, and builds the app ref into dist/.
// `css: 'injected'` keeps the component styles in the client script: an app ships one file.
export default defineConfig({
  plugins: [svelte({ configFile: false, compilerOptions: { css: 'injected' } }), vos()]
})
