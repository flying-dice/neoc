<script lang="ts">
  import { bridgeReader } from './lib/reader'
  import { Telemetry, type Clipboard, type ProbeRpc } from './lib/telemetry.svelte'
  import TelemetryPage from './pages/TelemetryPage.svelte'

  // The composition root: the one place the state meets the real host page, backend (the app's private method on the
  // bus, the global `RPC`), tracing (the global `vos.tracing`) and the clipboard (the app declares `clipboardWrite`). Each
  // window has its own.
  const rpc: ProbeRpc = { probe: (params) => RPC.send_request('telemetry', '_probe', params) }
  // The write begins in the click, with the text still to come (WebKit is strict about when the clipboard may be written);
  // where there is no `ClipboardItem` it waits for the text and writes it plainly.
  const clipboard: Clipboard = {
    write: (text) =>
      typeof ClipboardItem === 'undefined'
        ? text.then((t) => navigator.clipboard.writeText(t))
        : navigator.clipboard.write([new ClipboardItem({ 'text/plain': text.then((t) => new Blob([t], { type: 'text/plain' })) })])
  }
  const telemetry = new Telemetry(bridgeReader(), rpc, { tracer: vos.tracing, clipboard })
</script>

<TelemetryPage {telemetry} />
