/**
 * The managed renderer's public surface (what the shell and wave B2 import). Everything under `view/` is the
 * client half of the server-driven UI; the shell stays as it is until it opens windows through here.
 *
 *   const session = new Session({ transport: createWebSocketTransport({ url: defaultRpcUrl(location) }), createStore: reactiveStore })
 *   session.start()
 *   const view = await session.openView({ app: 'notes', window: 'main', windowId: 'w1', size: { w: 960, h: 640 } })
 *   // <ViewHost {view} app="notes" onretry={() => session.retryNow()} />
 */
export { Session, type SessionOptions, type SessionState } from './core/session'
export {
  ViewCore,
  type Binding,
  type EmitResult,
  type Scope,
  type ViewEffects,
  type ViewLink
} from './core/view'
export { plainStore, type ViewStore } from './core/store'
export { createWebSocketTransport, defaultRpcUrl } from './rpc/websocket'
export {
  createRegistry,
  ComponentRegistry,
  type AdapterProps,
  type ComponentSpec
} from './registry'
export { defaultRegistry, registerStarterKit } from './kit'
export { createDomEffects } from './effects'
export { reactiveStore } from './store.svelte'
export { default as ViewHost } from './organisms/ViewHost.svelte'
export * as protocol from './protocol'
