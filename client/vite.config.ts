/**
 * The AgentOS PWA client (Vite + Svelte).
 *
 *   index.html   the shell: it talks to the Rust host over /rpc (WebSocket JSON-RPC) and pairs through
 *                /session and /pair
 *
 *   bun run dev        Vite on :5173, proxying the host's routes to the host on :7400 (start it with
 *                      AGENTOS_DEV_CLIENT_PORT=5173 so its Origin guard accepts the dev client)
 *   bun run build      → dist/ (the host serves it)
 */
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const here = import.meta.dirname
const host = process.env.AGENTOS_HOST ?? 'http://127.0.0.1:7400'

export default defineConfig({
  root: resolve(here, 'src/renderer'),
  publicDir: resolve(here, 'src/renderer/public'),
  plugins: [svelte({ configFile: resolve(here, 'svelte.config.mjs') })],
  build: {
    target: 'esnext',
    outDir: resolve(here, 'dist'),
    emptyOutDir: true,
    rollupOptions: { input: { index: resolve(here, 'src/renderer/index.html') } }
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/rpc': { target: host.replace(/^http/, 'ws'), ws: true },
      '/session': host,
      '/pair': host,
      '/agui': host,
      '/mcp': host
    }
  },
  preview: { port: 6200, strictPort: true }
})
