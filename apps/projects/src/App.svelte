<script lang="ts">
  import { api } from '@projects/rpc'
  import { host } from '@vos/host'
  import { Projects } from './lib/projects.svelte'
  import ProjectsPage from './pages/ProjectsPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const projects = new Projects(api, host)
</script>

<ProjectsPage {projects} />
