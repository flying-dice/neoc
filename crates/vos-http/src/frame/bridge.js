// The host's side of an app's frame (served at /frame/bridge.js to every frame, before the app's client).
//
// The client is a classic script that defines `vosApp` with `mount(el, { rpc, host })`, which returns a function
// that unmounts it. This script mounts it once the page has loaded and gives it the only two things it may use:
//
//   rpc(method, args)   calls the handler `method` of the app's own backend; resolves with its result, or rejects
//                       with an Error whose message is the handler's
//   host.on(name, fn)   hears an event: one the backend sent with `app.emit(name, data)`, or the host's own
//                       (`host:theme`, "light" or "dark"); returns a function that stops listening
//   host.theme          "light" or "dark", the shell's appearance now
//
// Both travel by postMessage to the shell that embeds the frame, which knows which app the frame belongs to and
// answers; nothing the client says names an app. A message is only believed when it comes from the shell.
(function () {
  'use strict'

  var pending = new Map()
  var listeners = new Map()
  var next = 1
  var unmount = null

  function post(message) {
    parent.postMessage(message, '*')
  }

  function rpc(method, args) {
    return new Promise(function (resolve, reject) {
      var id = next++
      pending.set(id, { resolve: resolve, reject: reject })
      try {
        post({ t: 'rpc', id: id, method: String(method), args: args === undefined ? null : args })
      } catch (error) {
        pending.delete(id)
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    })
  }

  function on(name, fn) {
    var set = listeners.get(name)
    if (!set) listeners.set(name, (set = new Set()))
    set.add(fn)
    return function () {
      set.delete(fn)
    }
  }

  var host = {
    on: on,
    get theme() {
      return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light'
    }
  }

  function setTheme(theme) {
    var value = theme === 'dark' ? 'dark' : 'light'
    document.documentElement.dataset.theme = value
    document.documentElement.style.colorScheme = value
  }

  function dispatch(name, data) {
    var set = listeners.get(name)
    if (!set) return
    Array.from(set).forEach(function (fn) {
      try {
        fn(data)
      } catch (error) {
        console.error('vos: an event handler failed', error)
      }
    })
  }

  window.addEventListener('message', function (e) {
    if (e.source !== parent) return
    var m = e.data
    if (typeof m !== 'object' || m === null) return
    if (m.t === 'res') {
      var call = pending.get(m.id)
      if (!call) return
      pending.delete(m.id)
      if (typeof m.error === 'string') call.reject(new Error(m.error))
      else call.resolve(m.result)
    } else if (m.t === 'event' && typeof m.name === 'string') {
      if (m.name === 'host:theme') setTheme(m.data)
      dispatch(m.name, m.data)
    }
  })

  function fail(message) {
    console.error('vos: ' + message)
    var root = document.getElementById('root')
    if (root) root.textContent = 'This app could not start: ' + message
    post({ t: 'failed', message: message })
  }

  function start() {
    var app = window.vosApp
    var root = document.getElementById('root')
    if (!app || typeof app.mount !== 'function') return fail('the client does not define vosApp.mount')
    try {
      var result = app.mount(root, { rpc: rpc, host: host })
      Promise.resolve(result).then(
        function (stop) {
          if (typeof stop === 'function') unmount = stop
          post({ t: 'ready' })
        },
        function (error) {
          fail(error instanceof Error ? error.message : String(error))
        }
      )
    } catch (error) {
      fail(error instanceof Error ? error.message : String(error))
    }
  }

  window.addEventListener('pagehide', function () {
    if (unmount) unmount()
  })

  if (document.readyState === 'complete') start()
  else window.addEventListener('load', start)
})()
