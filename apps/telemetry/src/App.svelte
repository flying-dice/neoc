<script lang="ts">
  import { api } from '@telemetry/rpc'
  import { span } from '@vos/tracing'
  import { bridgeReader } from './lib/reader'
  import { Telemetry } from './lib/telemetry.svelte'
  import TelemetryPage from './pages/TelemetryPage.svelte'

  // The composition root: the one place the state meets the real host page, backend and tracing. Each window has its own.
  const telemetry = new Telemetry(bridgeReader(), api, { tracer: { span } })
</script>

<TelemetryPage {telemetry} />
