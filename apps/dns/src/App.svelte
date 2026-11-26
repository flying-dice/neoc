<script lang="ts">
  import { Dns, type DnsEvents, type DnsRpc } from './lib/dns.svelte'
  import DnsPage from './pages/DnsPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: DnsRpc = {
    overview: () => RPC.send_request('dns', '_overview'),
    connect: (params) => RPC.send_request('dns', '_connect', params),
    setEnabled: (params) => RPC.send_request('dns', '_set_enabled', params),
    apply: () => RPC.send_request('dns', '_apply'),
    saveSettings: (params) => RPC.send_request('dns', '_save_settings', params),
    addRecord: (params) => RPC.send_request('dns', '_create_record', params),
    removeRecord: (params) => RPC.send_request('dns', '_delete_record', params),
    logs: (params) => RPC.send_request('dns', '_logs', params)
  }
  const events: DnsEvents = {
    on: (name, fn) => (name === 'changed' ? RPC.on_notification('dns', 'changed', fn) : vos.host.on(name, fn))
  }
  const dns = new Dns(rpc, events)
</script>

<DnsPage {dns} />
