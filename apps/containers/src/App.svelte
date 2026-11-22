<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Containers } from './lib/containers.svelte'
  import ContainersPage from './pages/ContainersPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const containers = new Containers(api, host)
</script>

<ContainersPage {containers} />
