<script lang="ts">
  import { api } from '@containers/rpc'
  import { host } from '@vos/host'
  import { Containers } from './lib/containers.svelte'
  import ContainersPage from './pages/ContainersPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const containers = new Containers(api, host)
</script>

<ContainersPage {containers} />
