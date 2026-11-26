<script lang="ts">
  import { Processes, type ProcessesEvents, type ProcessesRpc } from './lib/processes.svelte'
  import ProcessesPage from './pages/ProcessesPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: ProcessesRpc = {
    snapshot: (params) => RPC.send_request('processes', '_snapshot', params),
    select: (params) => RPC.send_request('processes', '_select', params),
    release: (params) => RPC.send_request('processes', '_release', params),
    terminate: (params) => RPC.send_request('processes', '_terminate', params)
  }
  const events: ProcessesEvents = { on: (name, fn) => vos.host.on(name, fn) }
  const processes = new Processes(rpc, events)
</script>

<ProcessesPage {processes} />
