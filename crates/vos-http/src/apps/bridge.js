// The app's side of the bridge: served at /apps/bridge.js, injected into the app's `index.html` by the build.
//
// An app's page runs in a sandboxed frame and talks to the host page that holds it (the shell, or the standalone
// wrapper) over postMessage, as JSON-RPC 2.0 in a dialect of MCP Apps (SEP-1865, 2026-01-26):
//
//   app  -> host   ui/initialize (request), ui/notifications/initialized, ui/request-display-mode, ui/open-link,
//                  ping, vos/failed (the page could not start), and the bus and tracing (`vos/...`)
//   host -> app    ui/notifications/host-context-changed, ui/resource-teardown (request), and vos/event
//
// `ui/` names are the specification's; what is vos's own is under `vos/`; a page reaches its backend (and another
// app's) through the bus, `vos/request`. The host knows which app a frame is from the frame itself, never from a
// message.
//
// This script defines two globals, the page's whole API, with nothing to import. An app's build only adds types for
// them (the apps' requests and notifications, by app id). `RPC` is the RPC bus, the same four functions, in the same
// house style (snake case), as the global `RPC` of an app's backend:
//
//   RPC.send_request(app, method, params?)  a request to an app (this one's id too): every handler registered for
//                            the method, its backend's or an open page's, is asked; the first answer wins. Rejects
//                            with an `RPC.RpcError`
//   RPC.on_request(method, fn)  serve one of this app's methods from the page: `fn(params, { signal })` answers (it
//                            may return a promise); `signal` aborts when another handler answered first. A public
//                            method is an agent tool too, described by the app's contract. Returns a function that
//                            stops serving
//   RPC.send_notification(name, params?)  send one of this app's notifications, to whoever listens
//   RPC.on_notification(app, name, fn)  hear an app's notification (this one's, or another app's public one); returns
//                            a function that stops listening
//   RPC.RpcError             what a failed request rejects with: `code`, `message`, `data`
//
// `vos` is the rest:
//
//   vos.host                 the host page: theme, context, capabilities, hosted, ready, on('host:...'), on_context,
//                            on_teardown, open_link(url), request_display_mode(mode)
//   vos.tracing              spans and events, after Rust's `tracing`, and `query(params)`: the Telemetry app's read of
//                            what was recorded (the host answers that app's frame only)
//
// Tracing (NEOC-521 POC): each request (`RPC.send_request`) is a span, a child of the span the page is in when it makes the call (else
// the root of a trace of its own), and its W3C `traceparent` goes with the call (`_meta`, next to `params`) so the
// host's and the backend's spans join it. A page opens spans of its own (`span(name, fields).in_scope(fn)`) and writes
// events (`info(message, fields)`); errors after the start, unhandled rejections and a failed start are error events.
// Records are sent to the host page in batches (`vos/telemetry`): every 2 s, at 50 records, and when the page goes.
//
// The span the page is in: the page has no async context (a JavaScript engine does not carry one across `await`), so
// `in_scope` makes its span current for what its function does before it first waits, and, while its promise is
// pending, for any call or event outside every other scope as long as it is the only scope pending (with two pending,
// which one a later call belongs to is not known, so it belongs to neither). An event in no span joins the trace of the
// call that answered last, if it answered under a second before (a render that fails on what a call returned).
//
// The page's `data-theme` and `color-scheme` are set before first paint from the `theme` the page was opened with
// (the host's appearance), and corrected by the handshake.
(function () {
  'use strict'

  var PROTOCOL_VERSION = '2026-01-26'
  var pending = new Map()
  var eventListeners = new Map()
  // another app's notifications, by `<app> <name>`, and the apps the host was asked for
  var appListeners = new Map()
  var subscribed = new Set()
  // the app's methods this page serves on the bus, and the requests it is answering, by call id
  var requestHandlers = new Map()
  var serving = new Map()
  var contextListeners = new Set()
  var teardownListeners = new Set()
  var next = 1
  var started = false
  var resolveReady
  var rejectReady
  // the host context now (theme, displayMode, locale, timeZone, ...) and what the host offers (MCP Apps' hostCapabilities)
  var context = {}
  var capabilities = {}
  // the handshake is done (`context` and `capabilities` are set)
  var ready = new Promise(function (resolve, reject) {
    resolveReady = resolve
    rejectReady = reject
  })
  // an app that never reads `ready` must not report an unhandled rejection for it
  ready.catch(function () {})
  var vos = {}
  // the RPC bus
  var RPC = {}

  function post(message) {
    parent.postMessage(Object.assign({ jsonrpc: '2.0' }, message), '*')
  }

  // ---- tracing ---------------------------------------------------------------------------------------------------
  var spans = []
  var logs = []
  var flushTimer = null
  /** The call that answered last: `{ traceId, spanId, endMs }`. */
  var lastCall = null
  var CORRELATE_MS = 1000
  var LEVELS = ['trace', 'debug', 'info', 'warn', 'error']
  /** The span whose function is running now (`in_scope`), innermost last. */
  var stack = []
  /** The scopes whose function returned a promise that has not settled. */
  var pendingScopes = new Set()

  function hex(bytes) {
    var a = new Uint8Array(bytes)
    crypto.getRandomValues(a)
    return Array.from(a, function (b) { return (b < 16 ? '0' : '') + b.toString(16) }).join('')
  }
  function nowMs() {
    return performance.timeOrigin + performance.now()
  }
  function flush() {
    if (flushTimer !== null) clearTimeout(flushTimer)
    flushTimer = null
    if (spans.length + logs.length === 0) return
    var batch = { spans: spans.splice(0), logs: logs.splice(0) }
    try {
      post({ method: 'vos/telemetry', params: batch })
    } catch (error) {
      // the host is gone, or a record is not cloneable: tracing is never the app's problem
    }
  }
  function queue(list, record) {
    list.push(record)
    if (spans.length + logs.length >= 50) flush()
    else if (flushTimer === null) flushTimer = setTimeout(flush, 2000)
  }
  function plainFields(fields) {
    if (fields === undefined || fields === null) return {}
    try {
      var copy = JSON.parse(JSON.stringify(fields))
      return typeof copy === 'object' && copy !== null && !Array.isArray(copy) ? copy : { value: copy }
    } catch (error) {
      return {}
    }
  }
  /** The span the page is in now: the innermost running scope, else the one pending scope (see the header). */
  function currentSpan() {
    if (stack.length > 0) return stack[stack.length - 1]
    if (pendingScopes.size === 1) return pendingScopes.values().next().value
    return null
  }
  function recentCall() {
    return lastCall && nowMs() - lastCall.endMs < CORRELATE_MS ? lastCall : null
  }
  function recordEvent(level, body, fields) {
    var span = currentSpan()
    var call = span ? null : recentCall()
    var at = span || call
    queue(logs, {
      timeMs: nowMs(),
      severity: LEVELS.indexOf(level) >= 0 ? level : 'info',
      body: String(body).slice(0, 2000),
      source: 'browser',
      traceId: at ? at.traceId : undefined,
      spanId: at ? at.spanId : undefined,
      attributes: Object.assign(plainFields(fields), call ? { 'vos.correlated': 'last-call' } : {})
    })
  }
  function recordError(error, fields) {
    var e = error instanceof Error ? error : null
    recordEvent(
      'error',
      e ? e.message || String(e) : String(error),
      Object.assign(
        { 'exception.type': e ? e.name : typeof error, 'exception.stacktrace': e && e.stack ? String(e.stack).slice(0, 2000) : undefined },
        plainFields(fields)
      )
    )
  }

  /** A span: opened now, a child of the span the page is in (else the root of a new trace); ended once. */
  function Span(name, fields, options) {
    var parentSpan = currentSpan()
    this.name = String(name)
    this.traceId = parentSpan ? parentSpan.traceId : hex(16)
    this.spanId = hex(8)
    this.parentSpanId = parentSpan ? parentSpan.spanId : undefined
    this.startMs = nowMs()
    this.attributes = plainFields(fields)
    var level = options && options.level
    if (level && level !== 'info' && LEVELS.indexOf(level) >= 0) this.attributes['span.level'] = level
    this.status = 'unset'
    this.ended = false
  }
  Span.prototype.record = function (key, value) {
    if (this.ended) return
    var one = {}
    one[String(key)] = value
    Object.assign(this.attributes, plainFields(one))
  }
  Span.prototype.end = function (error) {
    if (this.ended) return
    this.ended = true
    if (error !== undefined) {
      this.status = 'error'
      this.attributes['error.message'] = String((error && error.message) || error).slice(0, 2000)
    } else if (this.status === 'unset') this.status = 'ok'
    queue(spans, {
      traceId: this.traceId,
      spanId: this.spanId,
      parentSpanId: this.parentSpanId,
      name: this.name,
      source: 'browser',
      startMs: this.startMs,
      endMs: nowMs(),
      status: this.status,
      attributes: this.attributes
    })
  }
  /** Run `body` in the span and end it when `body` returns, or when the promise it returns settles. */
  Span.prototype.in_scope = function (body) {
    var self = this
    stack.push(self)
    var result
    try {
      result = body()
    } catch (error) {
      stack.pop()
      self.end(error)
      throw error
    }
    stack.pop()
    if (result && typeof result.then === 'function') {
      pendingScopes.add(self)
      return Promise.resolve(result).then(
        function (value) {
          pendingScopes.delete(self)
          self.end()
          return value
        },
        function (error) {
          pendingScopes.delete(self)
          self.end(error)
          throw error
        }
      )
    }
    self.end()
    return result
  }

  vos.tracing = {
    span: function (name, fields, options) {
      return new Span(name, fields, options)
    },
    instrument: function (name, body, options) {
      return function () {
        var args = arguments
        var fields = options && typeof options.fields === 'function' ? options.fields.apply(null, args) : undefined
        var that = this
        return new Span(name, fields, options).in_scope(function () {
          return body.apply(that, args)
        })
      }
    },
    event: function (level, message, fields) {
      if (level === 'error' && message instanceof Error) recordError(message, fields)
      else recordEvent(level, message, fields)
    },
    trace: function (message, fields) {
      vos.tracing.event('trace', message, fields)
    },
    debug: function (message, fields) {
      vos.tracing.event('debug', message, fields)
    },
    info: function (message, fields) {
      vos.tracing.event('info', message, fields)
    },
    warn: function (message, fields) {
      vos.tracing.event('warn', message, fields)
    },
    error: function (message, fields) {
      vos.tracing.event('error', message, fields)
    },
    // what was recorded, for the Telemetry app (the host refuses every other app's frame)
    query: function (params) {
      return callHost('vos/telemetry/query', params)
    }
  }
  window.addEventListener('pagehide', flush)

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

  // what a failed request rejects with: a JSON-RPC error's `code`, `message` and `data`
  class RpcError extends Error {
    constructor(code, message, data) {
      super(message)
      this.name = 'RpcError'
      this.code = code
      this.data = data
    }
  }
  RPC.RpcError = RpcError

  function failureOf(error) {
    var message = error && typeof error.message === 'string' ? error.message : 'the call failed'
    return new RpcError(error && typeof error.code === 'number' ? error.code : -32603, message, error ? error.data : undefined)
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

  function callHost(method, params) {
    var name = String(method)
    // a request of the bus (`vos/request`) is traced, a child of the span the page is in; the rest of the dialect
    // (`ui/...`, `vos/...`) is not
    var bus = name === 'vos/request' && params && typeof params === 'object' ? params : null
    var span = bus
      ? new Span('rpc ' + bus.method, { 'rpc.service': String(bus.app), 'rpc.method': String(bus.method) })
      : null
    function end(error) {
      if (!span) return
      lastCall = { traceId: span.traceId, spanId: span.spanId, endMs: nowMs() }
      if (error) {
        span.attributes['error.code'] = error.code
      }
      span.end(error || undefined)
    }
    return new Promise(function (resolve, reject) {
      var id = next++
      pending.set(id, {
        resolve: function (result) { end(null); resolve(result) },
        reject: function (error) { end(error); reject(error) }
      })
      try {
        var message = { id: id, method: name, params: plainParams(bus ? String(bus.method) : name, params) }
        if (span) message._meta = { traceparent: '00-' + span.traceId + '-' + span.spanId + '-01' }
        post(message)
      } catch (error) {
        pending.delete(id)
        var failure = failureOf(error)
        end(failure)
        reject(failure)
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

  // an event of the page's own app (a `host:...` one too): `vos.host.on`
  function onEvent(name, fn) {
    return listen(eventListeners, name, fn)
  }
  // a public notification of another app (or this one's): the first listener for an app asks the host for its events
  function onApp(app, name, fn) {
    app = String(app)
    if (!subscribed.has(app)) {
      subscribed.add(app)
      post({ method: 'vos/subscribe', params: { app: app } })
    }
    return listen(appListeners, app + ' ' + name, fn)
  }
  RPC.on_request = function (method, fn) {
    method = String(method)
    if (requestHandlers.has(method)) throw new Error('This page already serves "' + method + '"')
    requestHandlers.set(method, fn)
    callHost('vos/handle', { method: method }).catch(function () {})
    return function () {
      if (requestHandlers.get(method) !== fn) return
      requestHandlers.delete(method)
      callHost('vos/unhandle', { method: method }).catch(function () {})
    }
  }
  RPC.send_notification = function (name, params) {
    return callHost('vos/notify', { name: String(name), params: params === undefined ? {} : params })
  }
  RPC.send_request = function (app, method, params) {
    return callHost('vos/request', { app: String(app), method: String(method), params: params === undefined ? {} : params })
  }
  RPC.on_notification = function (app, name, fn) {
    return onApp(app, name, function (params) {
      fn(params === undefined || params === null ? {} : params)
    })
  }

  // the host page that holds the app
  vos.host = {
    on: function (name, fn) {
      return onEvent(name, fn)
    },
    get theme() {
      // before the handshake the context is empty: the page's own data-theme (set before first paint) is the answer
      return (context.theme || document.documentElement.dataset.theme) === 'dark' ? 'dark' : 'light'
    },
    get context() {
      return context
    },
    get capabilities() {
      return capabilities
    },
    get hosted() {
      return Boolean(capabilities.experimental && capabilities.experimental.vos && capabilities.experimental.vos.ui)
    },
    get ready() {
      return ready.then(function () {})
    },
    on_context: function (fn) {
      return onContext(fn)
    },
    on_teardown: function (fn) {
      return onTeardown(fn)
    },
    open_link: function (url) {
      return callHost('ui/open-link', { url: url }).then(function () {})
    },
    request_display_mode: function (mode) {
      return callHost('ui/request-display-mode', { mode: mode }).then(function (r) {
        return r.mode
      })
    }
  }

  // a request of the bus for a method this page serves: answer it, unless another handler does first
  function serve(params) {
    var call = params.call
    var fn = requestHandlers.get(params.method)
    var answer = function (body) {
      body.call = call
      return callHost('vos/answer', body).catch(function (error) {
        // too late: another handler answered first, or the request was cancelled
        var running = serving.get(call)
        if (running) running.abort(error)
      })
    }
    if (!fn) return answer({ error: { code: -32601, message: 'This page no longer serves ' + params.method } })
    var control = new AbortController()
    serving.set(call, control)
    var settle = function () {
      serving.delete(call)
    }
    Promise.resolve()
      .then(function () {
        return fn(params.params === undefined ? {} : params.params, { signal: control.signal })
      })
      .then(
        function (result) {
          if (control.signal.aborted) return settle()
          return answer({ result: result === undefined ? null : result }).then(settle)
        },
        function (error) {
          if (control.signal.aborted) return settle()
          var code = error && typeof error.code === 'number' ? error.code : -32010
          return answer({ error: { code: code, message: String((error && error.message) || error) } }).then(settle)
        }
      )
  }

  function onContext(fn) {
    contextListeners.add(fn)
    return function () {
      contextListeners.delete(fn)
    }
  }
  function onTeardown(fn) {
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
    context = Object.assign({}, context, change)
    if (change.theme !== undefined) setTheme(change.theme)
    each(contextListeners, context)
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
          // an app's event carries its `app` (the page's own app's too, marked `own`): heard by `RPC.on_notification`,
          // and the own app's by `vos.host.on` as well
          if (typeof params.app === 'string') {
            var heard = appListeners.get(params.app + ' ' + params.name)
            if (heard) each(heard, params.data)
          }
          if (params.own === true || typeof params.app !== 'string') {
            var set = eventListeners.get(params.name)
            if (set) each(set, params.data)
          }
        } else if (m.method === 'vos/serve' && params && typeof params.method === 'string') serve(params)
        else if (m.method === 'vos/settled' && params) {
          // another handler answered first: what this page shows for the request goes
          var running = serving.get(params.call)
          if (running) running.abort(new Error('Answered elsewhere'))
          serving.delete(params.call)
        }
        return
      }
      if (m.method === 'ping') return post({ id: m.id, result: {} })
      if (m.method === 'ui/resource-teardown') {
        flush()
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
    recordEvent('error', 'the app could not start: ' + message, { 'vos.phase': 'startup' })
    flush()
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

  // after the start: an error or a rejection nobody handled is recorded (before it, `fail` says the page could not start)
  window.addEventListener('error', function (e) {
    if (!started) return
    recordError(e.error || e.message, { filename: e.filename, lineno: e.lineno, colno: e.colno })
  })
  window.addEventListener('unhandledrejection', function (e) {
    recordError(e.reason, { 'vos.unhandled': 'rejection' })
  })

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
    callHost('ui/initialize', {
      protocolVersion: PROTOCOL_VERSION,
      appInfo: { name: document.title || location.pathname, version: '1' },
      appCapabilities: { availableDisplayModes: ['inline', 'fullscreen'] }
    }).then(
      function (result) {
        started = true
        capabilities = (result && result.hostCapabilities) || {}
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
  Object.defineProperty(window, 'RPC', { value: RPC, configurable: false, writable: false })

  // The first thing the page says is `ui/initialize`, now, before any of the app's own scripts run: the host page takes it as the
  // page's hello (a document that never says it is a page the frame navigated itself to), however busy the app is after.
  start()
})()
