<script lang="ts">
  import { bridgeReader } from './lib/reader'
  import { Telemetry, type ProbeRpc } from './lib/telemetry.svelte'
  import TelemetryPage from './pages/TelemetryPage.svelte'

  // The composition root: the one place the state meets the real host page, backend (the app's private method on the
  // bus, the global `RPC`) and tracing (the global `vos.tracing`). Each window has its own.
  const rpc: ProbeRpc = { probe: (params) => RPC.send_request('telemetry', '_probe', params) }
  const telemetry = new Telemetry(bridgeReader(), rpc, { tracer: vos.tracing })
</script>

<TelemetryPage {telemetry} />
