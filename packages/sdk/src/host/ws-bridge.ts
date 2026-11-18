/**
 * The production `HostBridge`: JSON-RPC 2.0 over one WebSocket to the AgentOS
 * Bun server. Reconnects with backoff; requests made while (re)connecting wait
 * for the socket; server events fan out to listeners. It caches nothing:
 * consumers that hold server state refresh on `connection.onStatus('open')`
 * (after a reconnect they may have missed events).
 */
import { createRpcPeer, type RpcPeer } from '../rpc'
import type { ConnectionStatus, HostBridge, Unsubscribe } from './bridge'
import type {
  EventChannel,
  EventContract,
  InvokeChannel,
  InvokeContract,
  SendChannel,
  SendContract
} from './protocol'

export interface WsHostBridgeOptions {
  /** Max backoff between reconnect attempts, in ms (default 10s). */
  maxBackoffMs?: number
  /** Inject a WebSocket implementation (tests). */
  WebSocket?: typeof WebSocket
}

type Listener<T> = (payload: T) => void

/** The slice of `document` the bridge uses; absent outside browsers (e.g. Bun tests). */
interface FullscreenDocument {
  readonly fullscreenElement: unknown
  exitFullscreen(): Promise<void>
  readonly documentElement: { requestFullscreen(): Promise<void> }
  addEventListener(type: 'fullscreenchange', listener: () => void): void
  removeEventListener(type: 'fullscreenchange', listener: () => void): void
}
const browser = globalThis as { document?: FullscreenDocument; navigator?: { platform?: string; userAgent?: string } }

export function createWsHostBridge(url: string | URL, options: WsHostBridgeOptions = {}): HostBridge {
  const WS = options.WebSocket ?? globalThis.WebSocket
  const maxBackoff = options.maxBackoffMs ?? 10_000
  const listeners = new Map<EventChannel, Set<Listener<never>>>()
  const statusListeners = new Set<Listener<ConnectionStatus>>()

  let status: ConnectionStatus = 'connecting'
  let peer: RpcPeer | undefined
  let attempt = 0
  let opened: { promise: Promise<RpcPeer>; resolve(p: RpcPeer): void }

  const resetOpened = (): void => {
    let resolve!: (p: RpcPeer) => void
    opened = { promise: new Promise((r) => (resolve = r)), resolve }
  }
  resetOpened()

  const setStatus = (next: ConnectionStatus): void => {
    status = next
    statusListeners.forEach((l) => l(next))
  }

  const connect = (): void => {
    const ws = new WS(url)
    const current = createRpcPeer({
      send: (m) => ws.send(m),
      onNotification: (method, params) => {
        const set = listeners.get(method as EventChannel)
        set?.forEach((l) => (l as Listener<unknown>)(params[0]))
      },
      onError: (e) => console.warn('[agentos] rpc:', e)
    })
    ws.onopen = () => {
      attempt = 0
      peer = current
      setStatus('open')
      opened.resolve(current)
    }
    ws.onmessage = (e) => current.receive(String(e.data))
    ws.onclose = () => {
      current.close('Disconnected from the AgentOS server')
      if (peer === current) {
        peer = undefined
        resetOpened()
      }
      if (status === 'closed') return
      setStatus('reconnecting')
      const delay = Math.min(maxBackoff, 250 * 2 ** attempt++)
      setTimeout(connect, delay)
    }
  }
  connect()

  const invoke = async <C extends InvokeChannel>(
    channel: C,
    ...args: InvokeContract[C]['args']
  ): Promise<InvokeContract[C]['result']> => {
    const p = peer ?? (await opened.promise)
    // Server errors arrive as RpcError (an Error carrying the server's message and code).
    return p.request<InvokeContract[C]['result']>(channel, args)
  }

  const send = <C extends SendChannel>(channel: C, ...args: SendContract[C]): void => {
    if (peer) peer.notify(channel, args)
    else void opened.promise.then((p) => p.notify(channel, args))
  }

  const on = <C extends EventChannel>(channel: C, listener: Listener<EventContract[C]>): Unsubscribe => {
    const set = listeners.get(channel) ?? new Set()
    listeners.set(channel, set)
    set.add(listener as Listener<never>)
    return () => set.delete(listener as Listener<never>)
  }

  const fullscreenTarget = (): FullscreenDocument | undefined => browser.document

  return {
    platform: browser.navigator?.platform || 'web',
    versions: Object.freeze({ client: 'pwa', browser: browser.navigator?.userAgent ?? '' }),
    connection: {
      get status() {
        return status
      },
      onStatus: (l) => {
        statusListeners.add(l)
        return () => statusListeners.delete(l)
      }
    },
    host: { info: () => invoke('host:info') },
    window: {
      isFullScreen: async () => !!fullscreenTarget()?.fullscreenElement,
      toggleFullScreen: () => {
        const doc = fullscreenTarget()
        if (!doc) return
        if (doc.fullscreenElement) void doc.exitFullscreen()
        else void doc.documentElement.requestFullscreen().catch(() => {})
      },
      onFullScreenChange: (listener) => {
        const doc = fullscreenTarget()
        if (!doc) return () => {}
        const handler = (): void => listener(!!doc.fullscreenElement)
        doc.addEventListener('fullscreenchange', handler)
        return () => doc.removeEventListener('fullscreenchange', handler)
      }
    },
    apps: {
      list: () => invoke('apps:list'),
      known: () => invoke('apps:known'),
      install: (appId) => invoke('apps:install', appId),
      uninstall: (appId) => invoke('apps:uninstall', appId),
      onChanged: (l) => on('apps:changed', (e) => l(e.apps)),
      invoke: (appId, channel, payload) => invoke('apps:invoke', appId, channel, payload),
      onEvent: (l) => on('apps:event', l),
      onOpenRequest: (l) => on('apps:open', (e) => l(e.appId))
    },
    tools: {
      list: () => invoke('tools:list'),
      call: (id, input = {}) => invoke('tools:call', id, input)
    },
    agents: {
      list: () => invoke('agents:list'),
      newSession: (agentId) => invoke('agents:new-session', agentId),
      prompt: (agentId, sessionId, text) => invoke('agents:prompt', agentId, sessionId, text),
      cancel: (agentId, sessionId) => invoke('agents:cancel', agentId, sessionId),
      setMode: (agentId, sessionId, modeId) => invoke('agents:set-mode', agentId, sessionId, modeId),
      setConfig: (agentId, sessionId, configId, value) =>
        invoke('agents:set-config', agentId, sessionId, configId, value),
      respondPermission: (requestId, optionId) => send('agents:permission-response', requestId, optionId),
      onUpdate: (l) => on('agents:update', l),
      onPermission: (l) => on('agents:permission', l),
      onExit: (l) => on('agents:exit', l)
    }
  }
}
