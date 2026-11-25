<script lang="ts">
  import { api } from '@dns/rpc'
  import { host } from '@vos/host'
  import { Dns } from './lib/dns.svelte'
  import DnsPage from './pages/DnsPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const dns = new Dns(api, host)
</script>

<DnsPage {dns} />
