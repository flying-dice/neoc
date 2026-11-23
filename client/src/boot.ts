/**
 * The vos PWA entry (`index.html`): ask the host whether this browser is paired, show the pairing screen or
 * connect one session over `/rpc`, and start the shell.
 *
 *   ?theme=light|dark   force the appearance (tests, screenshots); such a page stores no settings
 *
 * `window.__vos` exposes the shell's stores for tests and the research panel (the window manager, the app
 * definitions, the host state); nothing in the product reads it.
 */
import { mount } from 'svelte'
import '@vos/ui/styles.css'
import './app.css'
import App from './App.svelte'
import { apps } from './lib/apps.svelte'
import { host } from './lib/host.svelte'
import { settings } from './lib/settings.svelte'
import { preventZoom } from './shell/noZoom'
import { windowManager } from './lib/windowManager.svelte'

declare global {
  interface Window {
    __vos: {
      host: typeof host
      wm: typeof windowManager
      settings: typeof settings
      apps: typeof apps
    }
  }
}

const theme = new URLSearchParams(location.search).get('theme')
if (theme === 'light' || theme === 'dark') settings.forceTheme(theme)

// Offline shell + installability. Dev serves modules live, so only register in production builds.
if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  void navigator.serviceWorker
    .register('/sw.js')
    .catch((e: unknown) => console.warn('[vos] service worker:', e))
}

preventZoom()
window.__vos = { host, wm: windowManager, settings, apps }
mount(App, { target: document.getElementById('app')! })
void host.start().then(() => {
  document.documentElement.dataset.ready = 'true'
})
