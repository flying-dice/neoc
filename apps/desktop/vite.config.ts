/**
 * The AgentOS PWA client (Vite + Svelte).
 *
 *   index.html   the real shell, talking to the Bun server over /rpc (WebSocket JSON-RPC)
 *   web.html     the same shell on the in-memory host (Playwright, research panel, quick pokes)
 *
 *   bun run dev        Vite on :5173, proxying /rpc /bundles /registry /mcp to the server on :7000
 *   bun run dev:web    web mode on :6200 (no server needed)
 *   bun run build      → dist/ (the server serves it)
 */
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const here = import.meta.dirname
const server = process.env.AGENTOS_SERVER ?? 'http://127.0.0.1:7000'

export default defineConfig({
  root: resolve(here, 'src/renderer'),
  publicDir: resolve(here, 'src/renderer/public'),
  plugins: [svelte({ configFile: resolve(here, 'svelte.config.mjs') })],
  build: {
    target: 'esnext',
    outDir: resolve(here, 'dist'),
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: resolve(here, 'src/renderer/index.html'),
        web: resolve(here, 'src/renderer/web.html')
      }
    }
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/rpc': { target: server.replace(/^http/, 'ws'), ws: true },
      '/bundles': server,
      '/registry': server,
      '/mcp': server
    }
  },
  preview: { port: 6200, strictPort: true }
})
