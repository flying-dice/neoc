/**
 * vos service worker: makes the shell installable and opens it offline.
 *  • Navigations: network first, falling back to the cached shell.
 *  • Hashed build assets (immutable URLs): cache first.
 *  • Never touches /rpc (WebSocket), /mcp (always live).
 */
const CACHE = 'vos-shell-v1'
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
  if (url.pathname === '/rpc' || url.pathname === '/mcp') return

  if (event.request.mode === 'navigate') {
    event.respondWith(fetch(event.request).catch(() => caches.match('/index.html')))
    return
  }
  if (url.pathname.startsWith('/assets/')) {
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
