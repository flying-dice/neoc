/**
 * The vos PWA client (Vite + Svelte).
 *
 *   index.html   the shell: it talks to the Rust host over /rpc (WebSocket JSON-RPC) and pairs through
 *                /session and /pair
 *
 *   bun run dev        Vite on :5173, proxying the host's routes to the host on :7400 (start it with
 *                      VOS_DEV_CLIENT_PORT=5173 so its Origin guard accepts the dev client)
 *   bun run build      → dist/ (the host serves it)
 */
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const here = import.meta.dirname
const host = process.env.VOS_HOST ?? 'http://127.0.0.1:7400'

export default defineConfig({
  root: here,
  publicDir: resolve(here, 'public'),
  plugins: [svelte({ configFile: resolve(here, 'svelte.config.mjs') })],
  build: {
    target: 'esnext',
    outDir: resolve(here, 'dist'),
    emptyOutDir: true,
    rollupOptions: {
      // `wrapper.js` is the script of the host page of a standalone app (`GET /apps/<id>`): the host writes that page
      // and names the script, so it has a fixed name
      input: {
        index: resolve(here, 'index.html'),
        wrapper: resolve(here, 'src/wrapper/main.svelte.ts')
      },
      output: {
        entryFileNames: (chunk) =>
          chunk.name === 'wrapper' ? 'wrapper.js' : 'assets/[name]-[hash].js'
      }
    }
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/rpc': { target: host.replace(/^http/, 'ws'), ws: true },
      '/session': host,
      '/pair': host,
      '/agui': host,
      '/apps': host,
      '/wrapper.js': host,
      '/mcp': host
    }
  },
  preview: { port: 6200, strictPort: true }
})
