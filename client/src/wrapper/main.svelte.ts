/**
 * The standalone host page of an app: the script of `GET /apps/<id>` (the host writes the page, which is one iframe).
 *
 * Opened on its own, an app is not in the shell, so nothing here draws: no window, no sheets, no alerts, no toasts.
 * This script is the host side of the same bridge the shell makes for a window (`shell/hostBridge.ts`), over the same
 * registry of host services (`shell/hostServices.ts`): the app's backend calls and events travel over the device's own
 * `/rpc` connection, and the handshake answers with the host context drawn from the system settings. What the shell
 * draws or arranges is not registered here, so it is absent in the handshake and an app can see that.
 *
 * The app id is the path this page was served on (`/apps/<id>`), never anything a message says.
 */
import { settings } from '../lib/settings.svelte'
import { createHostBridge, eventFor, type HostBridge } from '../shell/hostBridge'
import { callWhenConnected, createHostChannel, type CallOptions } from '../shell/channel'
import { contextChange, hostContextOf, type HostContext } from '../shell/hostContext'
import { focusFrame } from '../shell/frameFocus'
import { baseServices } from '../shell/hostServices'
import { preventZoom } from '../shell/noZoom'
import { showAppIcon } from './appIcon'
import { createWebSocketTransport, defaultRpcUrl } from '../shell/rpc/websocket'

const MODES = ['fullscreen'] as const

/** The app a path names: `/apps/<id>` with or without a trailing slash. */
export function appIdOf(pathname: string): string | null {
  return /^\/apps\/([a-z0-9][a-z0-9-]*)\/?$/.exec(pathname)?.[1] ?? null
}

/** The context the page tells the app: the system settings' appearance, and a display mode that is a fact of the page. */
const currentContext = (): HostContext =>
  hostContextOf({
    appearance: settings.appearance,
    displayMode: 'fullscreen',
    availableDisplayModes: [...MODES]
  })

/**
 * Is this page running from a home screen, with no browser around it? Safari says so with `navigator.standalone`
 * (a page added without a manifest matches no `display-mode` query there); other browsers with the media query.
 */
function isInstalled(): boolean {
  if ((navigator as Navigator & { standalone?: boolean }).standalone === true) return true
  return (
    typeof matchMedia === 'function' &&
    matchMedia('(display-mode: standalone), (display-mode: fullscreen)').matches
  )
}

export function start(root: Document = document): void {
  const iframe = root.getElementById('app')
  const appId = appIdOf(location.pathname)
  if (!(iframe instanceof HTMLIFrameElement) || appId === null) {
    root.body.textContent = 'This page is not an app.'
    return
  }
  // added to a home screen, the page is as tall as the screen (the host's style reads this class: see `WRAPPER_STYLE`)
  if (isInstalled()) root.documentElement.classList.add('installed')
  // the page is the app's own surface: it does not zoom, and its tab and home-screen icon are the app's
  preventZoom(root)
  void showAppIcon(root, iframe.dataset.icon, iframe.dataset.tint)

  const channel = createHostChannel(createWebSocketTransport({ url: defaultRpcUrl(location) }))
  channel.transport.connect()
  /** The same call the shell makes (`AppFrame`), waiting for the connection first: the options of the bridge's calls pass through. */
  const call = (
    method: string,
    params: Record<string, unknown>,
    options?: CallOptions
  ): Promise<unknown> => callWhenConnected(channel, method, params, options)

  const services = baseServices({
    context: currentContext,
    openLink: (url) => void window.open(url, '_blank', 'noopener,noreferrer')
    // no `setDisplayMode`: this page is always full screen, and `ui/request-display-mode` answers with that
  })
  let told = currentContext()
  const bridge: HostBridge = createHostBridge({
    appId,
    frame: () => iframe.contentWindow,
    post: (message) => iframe.contentWindow?.postMessage(message, '*'),
    call,
    services,
    context: () => (told = currentContext()),
    // opened on its own, the page has nothing else to hold the keys: the app takes them as soon as it is ready, as in a window
    onReady: () => void focusFrame(iframe),
    onFailed: (message) => console.error(`vos: ${appId} could not start: ${message}`)
  })
  window.addEventListener('message', (e) => bridge.receive(e))
  channel.onNotification((method, params) => {
    const event = eventFor(appId, method, params)
    if (event) bridge.event(event.name, event.data)
  })

  // the connection dropped and came back (a restart, the network): a change made while it was away reached nobody, so the
  // app is told it can fetch again (`host:reconnected`, a `vos/event` that the app hears with `host.on`)
  let wasOpen = false
  let dropped = false
  channel.transport.onStatus((status) => {
    if (status === 'open') {
      if (dropped) bridge.event('host:reconnected', null)
      wasOpen = true
      dropped = false
    } else if (wasOpen) dropped = true
  })

  // the host context, live: what the system settings say, as what changed
  $effect.root(() => {
    $effect(() => {
      // what shows around the frame (the safe areas of a phone) is in the appearance the app is in
      root.documentElement.style.colorScheme = settings.appearance
      const now = currentContext()
      const change = contextChange(told, now)
      told = now
      bridge.contextChanged(change)
    })
  })

  // the page is the frame's for as long as it lives: when it goes, the app may wrap up
  window.addEventListener('pagehide', () => void bridge.teardown('closed', 100))

  // load the app, in the appearance the settings give it now (the handshake corrects it if that changes)
  iframe.src = `${iframe.dataset.entry ?? `/apps/${appId}/static/`}?theme=${settings.appearance}`
}

if (typeof document !== 'undefined' && document.getElementById('app') !== null) start()
