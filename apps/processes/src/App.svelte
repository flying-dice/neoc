<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Processes } from './lib/processes.svelte'
  import ProcessesPage from './pages/ProcessesPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const processes = new Processes(api, host)
</script>

<ProcessesPage {processes} />
