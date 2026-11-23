// The app's side of the bridge: served at /apps/bridge.js, injected into the app's `index.html` by the build.
//
// An app's page runs in a sandboxed frame and talks to the host page that holds it (the shell, or the standalone
// wrapper) over postMessage, as JSON-RPC 2.0 in a dialect of MCP Apps (SEP-1865, 2026-01-26):
//
//   app  -> host   ui/initialize (request), ui/notifications/initialized, ui/request-display-mode, ui/open-link,
//                  ping, vos/failed (the page could not start), and any other method as the app's own backend call
//   host -> app    ui/notifications/host-context-changed, ui/resource-teardown (request), and vos/event
//
// `ui/` names are the specification's; what is vos's own is under `vos/`; the app's own methods (its OpenRPC
// contract) are plain names the host forwards to the app's backend. The host knows which app a frame is from the
// frame itself, never from a message.
//
// This script defines one global, `vos`, that the generated `app:rpc` module reads:
//
//   vos.ready                a promise: the handshake is done (`vos.context` and `vos.capabilities` are set)
//   vos.call(method, params) a JSON-RPC request to the host; rejects with an Error that carries `code` and `data`
//   vos.on(name, fn)         hear a `vos/event` (what the backend sent with `app.emit`); returns a function to stop
//   vos.onContext(fn)        hear a change of the host context; returns a function to stop
//   vos.onTeardown(fn)       run `fn` (may return a promise) when the host is about to remove the frame
//   vos.context              the host context now (theme, displayMode, locale, timeZone, ...)
//   vos.capabilities         what the host offers (MCP Apps' hostCapabilities)
//
// The page's `data-theme` and `color-scheme` are set before first paint from the `theme` the page was opened with
// (the host's appearance), and corrected by the handshake.
(function () {
  'use strict'

  var PROTOCOL_VERSION = '2026-01-26'
  var pending = new Map()
  var eventListeners = new Map()
  var contextListeners = new Set()
  var teardownListeners = new Set()
  var next = 1
  var started = false
  var resolveReady
  var rejectReady
  var vos = {
    context: {},
    capabilities: {},
    ready: new Promise(function (resolve, reject) {
      resolveReady = resolve
      rejectReady = reject
    })
  }
  // an app that never reads `ready` must not report an unhandled rejection for it
  vos.ready.catch(function () {})

  function post(message) {
    parent.postMessage(Object.assign({ jsonrpc: '2.0' }, message), '*')
  }

  function setTheme(theme) {
    var value = theme === 'dark' ? 'dark' : 'light'
    document.documentElement.dataset.theme = value
    document.documentElement.style.colorScheme = value
  }

  // an app is not a document to zoom: no pinch or double-tap scaling of its page (scrolling stays), as in the shell.
  // Safari's own gesture events scale a page whatever `touch-action` says, so they are refused too.
  document.documentElement.style.touchAction = 'pan-x pan-y'
  ;['gesturestart', 'gesturechange', 'gestureend'].forEach(function (gesture) {
    document.addEventListener(gesture, function (event) {
      event.preventDefault()
    })
  })

  // before first paint: the appearance the host opened the page with, else the browser's
  var opened = new URLSearchParams(location.search).get('theme')
  setTheme(
    opened === 'dark' || opened === 'light'
      ? opened
      : window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
  )

  function failureOf(error) {
    var message = error && typeof error.message === 'string' ? error.message : 'the call failed'
    var failure = new Error(message)
    failure.code = error && typeof error.code === 'number' ? error.code : -32603
    failure.data = error ? error.data : undefined
    return failure
  }

  // What goes over `postMessage` is structured-cloned, and a framework's reactive object (a Svelte 5 `$state` proxy, a Vue `reactive`)
  // cannot be: the arguments are plain JSON before they are posted (the contract is JSON-RPC, so anything that does not survive
  // `JSON.stringify` is not a valid argument), and when they are not, the error names the method.
  function plainParams(method, params) {
    if (params === undefined) return {}
    try {
      var text = JSON.stringify(params)
      if (text === undefined) throw new Error('it is not JSON')
      return JSON.parse(text)
    } catch (error) {
      throw new Error('The arguments of "' + method + '" are not JSON: ' + ((error && error.message) || error))
    }
  }

  vos.call = function (method, params) {
    return new Promise(function (resolve, reject) {
      var id = next++
      pending.set(id, { resolve: resolve, reject: reject })
      try {
        post({ id: id, method: String(method), params: plainParams(String(method), params) })
      } catch (error) {
        pending.delete(id)
        reject(failureOf(error))
      }
    })
  }

  function listen(map, name, fn) {
    var set = map.get(name)
    if (!set) map.set(name, (set = new Set()))
    set.add(fn)
    return function () {
      set.delete(fn)
    }
  }

  vos.on = function (name, fn) {
    return listen(eventListeners, name, fn)
  }
  vos.onContext = function (fn) {
    contextListeners.add(fn)
    return function () {
      contextListeners.delete(fn)
    }
  }
  vos.onTeardown = function (fn) {
    teardownListeners.add(fn)
    return function () {
      teardownListeners.delete(fn)
    }
  }

  function each(set, arg) {
    Array.from(set).forEach(function (fn) {
      try {
        fn(arg)
      } catch (error) {
        console.error('vos: a listener failed', error)
      }
    })
  }

  function mergeContext(change) {
    if (typeof change !== 'object' || change === null) return
    vos.context = Object.assign({}, vos.context, change)
    if (change.theme !== undefined) setTheme(change.theme)
    each(contextListeners, vos.context)
  }

  window.addEventListener('message', function (e) {
    if (e.source !== parent) return
    var m = e.data
    if (typeof m !== 'object' || m === null || m.jsonrpc !== '2.0') return
    var hasId = m.id !== undefined && m.id !== null
    if (typeof m.method === 'string') {
      var params = m.params
      if (!hasId) {
        if (m.method === 'ui/notifications/host-context-changed') mergeContext(params)
        else if (m.method === 'vos/event' && params && typeof params.name === 'string') {
          var set = eventListeners.get(params.name)
          if (set) each(set, params.data)
        }
        return
      }
      if (m.method === 'ping') return post({ id: m.id, result: {} })
      if (m.method === 'ui/resource-teardown') {
        var done = Array.from(teardownListeners).map(function (fn) {
          try {
            return Promise.resolve(fn(params))
          } catch (error) {
            return Promise.reject(error)
          }
        })
        Promise.all(done).then(
          function () {
            post({ id: m.id, result: {} })
          },
          function (error) {
            post({ id: m.id, error: { code: -32000, message: String((error && error.message) || error) } })
          }
        )
        return
      }
      return post({ id: m.id, error: { code: -32601, message: 'Method not found: ' + m.method } })
    }
    if (!hasId) return
    var call = pending.get(m.id)
    if (!call) return
    pending.delete(m.id)
    if (typeof m.error === 'object' && m.error !== null) call.reject(failureOf(m.error))
    else call.resolve(m.result)
  })

  // the text goes into the page's root element once the page has one (this script runs in the head)
  function show(text) {
    var root = document.getElementById('root') || document.body
    if (root) root.textContent = text
    else document.addEventListener('DOMContentLoaded', function () { show(text) })
  }

  var failed = false
  function fail(message) {
    failed = true
    console.error('vos: ' + message)
    show('This app could not start: ' + message)
    try {
      post({ method: 'vos/failed', params: { message: message } })
    } catch (error) {
      // the host is gone: nothing to tell
    }
    rejectReady(new Error(message))
  }

  // a page that throws before the handshake has not started: say so rather than sit there. The app's scripts are loaded with
  // `crossorigin` from the host, which answers with the header that lets the browser report the real message and place; a script
  // loaded without it is reported as `Script error.`, which says nothing, so say where to look instead.
  // The host's shortcut for search (Cmd+K, Ctrl+K) pressed while the keys are in this page: a key pressed in a frame
  // never reaches the page that holds it, so the host is told it was pressed. Whether it is the host's shortcut on this
  // platform is the host's to say; the key is the page's as well, as any key is.
  window.addEventListener('keydown', function (e) {
    if (String(e.key).toLowerCase() !== 'k' || !(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey || e.repeat || e.isComposing) return
    try {
      post({ method: 'vos/shortcut', params: { key: 'k', metaKey: !!e.metaKey, ctrlKey: !!e.ctrlKey } })
    } catch (error) {
      // the host is gone: nothing to tell
    }
  }, true)

  window.addEventListener('error', function (e) {
    if (started || failed) return
    var hidden = !e.message || e.message === 'Script error.'
    var where = e.filename
      ? ' (' + e.filename.split('/').slice(-2).join('/') + (e.lineno ? ':' + e.lineno : '') + ')'
      : ''
    fail(hidden ? 'the page script failed while it loaded (the browser console has the error)' : e.message + where)
  })

  // A form's navigation is never useful in an app's frame, so a submission the app's own handlers did not prevent is prevented
  // here, in the bubble phase on `window`, after theirs: an app that forgets `preventDefault` stays on its page. A
  // `<form method="dialog">` closes its dialog and is no navigation: it is left alone. (`submit` does not cross a shadow root, and
  // `form.submit()` fires no event: those still navigate, and the shell notices a page the frame left.)
  window.addEventListener('submit', function (e) {
    if (e.defaultPrevented) return
    var form = e.target
    var submitter = e.submitter
    var method =
      (submitter && submitter.getAttribute && submitter.getAttribute('formmethod')) ||
      (form && form.getAttribute && form.getAttribute('method')) ||
      ''
    if (String(method).toLowerCase() === 'dialog') return
    e.preventDefault()
  })

  function start() {
    // opened in a tab of its own rather than in a frame: there is no host page to hold the app
    if (parent === window) return fail('This page needs its host page: open the app at ' + location.pathname.replace(/static\/.*$/, ''))
    vos
      .call('ui/initialize', {
        protocolVersion: PROTOCOL_VERSION,
        appInfo: { name: document.title || location.pathname, version: '1' },
        appCapabilities: { availableDisplayModes: ['inline', 'fullscreen'] }
      })
      .then(
        function (result) {
          started = true
          vos.capabilities = (result && result.hostCapabilities) || {}
          mergeContext(result && result.hostContext)
          post({ method: 'ui/notifications/initialized', params: {} })
          resolveReady({ hostInfo: result && result.hostInfo })
        },
        function (error) {
          fail(error && error.message ? error.message : 'the host did not accept the page')
        }
      )
  }

  Object.defineProperty(window, 'vos', { value: vos, configurable: false, writable: false })

  // The first thing the page says is `ui/initialize`, now, before any of the app's own scripts run: the host page takes it as the
  // page's hello (a document that never says it is a page the frame navigated itself to), however busy the app is after.
  start()
})()
