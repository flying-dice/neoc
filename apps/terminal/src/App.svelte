<script lang="ts">
  import { Terminal, type TerminalEvents, type TerminalRpc } from './lib/terminal.svelte'
  import TerminalPage from './pages/TerminalPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: TerminalRpc = {
    open: (params) => RPC.send_request('terminal', '_open', params),
    write: (params) => RPC.send_request('terminal', '_write', params),
    resize: (params) => RPC.send_request('terminal', '_resize', params),
    ack: (params) => RPC.send_request('terminal', '_ack', params),
    replay: (params) => RPC.send_request('terminal', '_replay', params),
    close: (params) => RPC.send_request('terminal', '_close', params)
  }
  const events: TerminalEvents = {
    on: (name, fn) => (name === 'host:reconnected' ? vos.host.on(name, fn) : RPC.on_notification('terminal', name, fn))
  }
  const terminal = new Terminal(rpc, events)

  // The window is closing: end the shell. (A page that goes away without being asked, a reload or a closed browser,
  // cannot be counted on to say so: the backend ends a session it stops hearing from.)
  vos.host.on_teardown(() => terminal.close())
</script>

<svelte:window onpagehide={() => void terminal.close()} />

<TerminalPage {terminal} />
