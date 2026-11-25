<script lang="ts">
  import { api } from '@services/rpc'
  import { host } from '@vos/host'
  import { Services } from './lib/services.svelte'
  import ServicesPage from './pages/ServicesPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const services = new Services(api, host)
</script>

<ServicesPage {services} />
