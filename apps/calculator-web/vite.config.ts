import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig, type Plugin } from 'vite'

// The kit's two tags (the tokens and the web components, one copy served by the host at /kit/). The vos plugin does not
// inject them yet (NEOC-252's next pull request does); until then this app puts them in its own page, at the top of the head
// (before its own styles, which then win over the kit's base, and its own script). Remove this when the plugin injects them.
const kitTags = (): Plugin => ({
  name: 'calculator-web:kit-tags',
  transformIndexHtml: () => [
    { tag: 'link', attrs: { rel: 'stylesheet', href: '/kit/tokens.css' }, injectTo: 'head-prepend' },
    { tag: 'script', attrs: { src: '/kit/kit.js' }, injectTo: 'head-prepend' }
  ]
})

// `vos()` reads openrpc.json, generates the typed client and the Luau router into .app/, and builds the app ref into dist/.
export default defineConfig({
  plugins: [svelte({ configFile: false }), vos(), kitTags()]
})
