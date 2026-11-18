/**
 * The AgentOS PWA entry (`index.html`): connect to the Bun server over
 * JSON-RPC/WebSocket, install the bridge, then start the shell.
 *
 * Order matters: shared modules first (app bundles read them), then the bridge
 * (stores read `window.agentos` when first imported), then the shell.
 */
import './lib/sharedModules'
import { createWsHostBridge, installHostBridge } from '@agentos/sdk/host/client'

const rpc = new URL('/rpc', location.href)
rpc.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
installHostBridge(createWsHostBridge(rpc))

// Offline shell + installability. Dev serves modules live, so only register in production builds.
if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  void navigator.serviceWorker
    .register('/sw.js')
    .catch((e: unknown) => console.warn('[agentos] service worker:', e))
}

await import('./main')
