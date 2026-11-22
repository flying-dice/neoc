<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Agents } from './lib/agents.svelte'
  import AgentsPage from './pages/AgentsPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const agents = new Agents(api, host)
</script>

<AgentsPage {agents} />
