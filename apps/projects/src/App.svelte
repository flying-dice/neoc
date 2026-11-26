<script lang="ts">
  import { Projects, type ProjectsEvents, type ProjectsRpc } from './lib/projects.svelte'
  import ProjectsPage from './pages/ProjectsPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: ProjectsRpc = {
    workspace: () => RPC.send_request('projects', '_workspace'),
    addProject: (params) => RPC.send_request('projects', '_add_project', params),
    removeProject: (params) => RPC.send_request('projects', '_remove_project', params),
    addIssue: (params) => RPC.send_request('projects', '_create_issue', params),
    updateIssue: (params) => RPC.send_request('projects', '_update_issue', params),
    removeIssue: (params) => RPC.send_request('projects', '_remove_issue', params),
    addPage: (params) => RPC.send_request('projects', '_add_page', params),
    readPage: (params) => RPC.send_request('projects', '_get_page', params),
    writePage: (params) => RPC.send_request('projects', '_save_page', params),
    removePage: (params) => RPC.send_request('projects', '_remove_page', params)
  }
  const events: ProjectsEvents = {
    on: (name, fn) => (name === 'projects_changed' ? RPC.on_notification('projects', 'projects_changed', fn) : vos.host.on(name, fn))
  }
  const projects = new Projects(rpc, events)
</script>

<ProjectsPage {projects} />
