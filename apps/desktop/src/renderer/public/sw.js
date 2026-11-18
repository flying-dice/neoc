/**
 * AgentOS service worker: makes the shell installable and opens it offline.
 *  • Navigations: network first, falling back to the cached shell.
 *  • Hashed build assets and app bundles (immutable URLs): cache first.
 *  • Never touches /rpc (WebSocket), /mcp or /registry (always live).
 */
const CACHE = 'agentos-shell-v1'
const SHELL = ['/', '/index.html', '/manifest.webmanifest', '/icon-512.png']

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting())
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  )
})

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url)
  if (event.request.method !== 'GET' || url.origin !== location.origin) return
  if (url.pathname === '/rpc' || url.pathname === '/mcp' || url.pathname.startsWith('/registry/'))
    return

  if (event.request.mode === 'navigate') {
    event.respondWith(fetch(event.request).catch(() => caches.match('/index.html')))
    return
  }
  if (url.pathname.startsWith('/assets/') || url.pathname.startsWith('/bundles/')) {
    event.respondWith(
      caches.match(event.request).then(
        (hit) =>
          hit ||
          fetch(event.request).then((res) => {
            if (res.ok) {
              const copy = res.clone()
              void caches.open(CACHE).then((cache) => cache.put(event.request, copy))
            }
            return res
          })
      )
    )
  }
})
