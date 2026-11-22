<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Projects } from './lib/projects.svelte'
  import ProjectsPage from './pages/ProjectsPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const projects = new Projects(api, host)
</script>

<ProjectsPage {projects} />
