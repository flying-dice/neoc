<script lang="ts">
  import { Services, type ServicesEvents, type ServicesRpc } from './lib/services.svelte'
  import ServicesPage from './pages/ServicesPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: ServicesRpc = {
    overview: () => RPC.send_request('services', '_overview'),
    start: (params) => RPC.send_request('services', '_start', params),
    stop: (params) => RPC.send_request('services', '_stop', params),
    restart: (params) => RPC.send_request('services', '_restart', params),
    configure: (params) => RPC.send_request('services', '_configure', params),
    register: (params) => RPC.send_request('services', '_register', params),
    unregister: (params) => RPC.send_request('services', '_unregister', params)
  }
  const events: ServicesEvents = { on: (name, fn) => vos.host.on(name, fn) }
  const services = new Services(rpc, events)
</script>

<ServicesPage {services} />
