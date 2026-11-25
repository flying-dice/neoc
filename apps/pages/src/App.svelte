<script lang="ts">
  import { api } from '@pages/rpc'
  import { host } from '@vos/host'
  import { Pages } from './lib/pages.svelte'
  import SitesPage from './pages/SitesPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const pages = new Pages(api, host)
</script>

<SitesPage {pages} />
