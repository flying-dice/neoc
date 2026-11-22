import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig, type Plugin } from 'vite'

// The kit's two tags (the tokens and the web components, one copy served by the host at /kit/), as apps/calculator-web
// adds them: the vos plugin does not inject them yet (NEOC-298's Calculator story makes it do so). Until then this app puts
// them at the top of its own head, before its own styles and script; the stylesheet is read `crossorigin`, as Vite loads the
// app's own. Remove this when the plugin injects them.
const kitTags = (): Plugin => ({
  name: 'agents:kit-tags',
  transformIndexHtml: () => [
    {
      tag: 'link',
      attrs: { rel: 'stylesheet', href: '/kit/tokens.css', crossorigin: '' },
      injectTo: 'head-prepend'
    },
    { tag: 'script', attrs: { src: '/kit/kit.js' }, injectTo: 'head-prepend' }
  ]
})

// `vos()` reads openrpc.json, generates the typed client and the Luau router into .app/, and builds the app ref into dist/.
export default defineConfig({
  plugins: [svelte({ configFile: false }), vos(), kitTags()]
})
