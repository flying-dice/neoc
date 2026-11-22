<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Notebooks } from './lib/notebooks.svelte'
  import NotebooksPage from './pages/NotebooksPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const notebooks = new Notebooks(api, host)

  // what was written a moment ago is sent before the host takes the page away
  host.onTeardown(async () => {
    await notebooks.flush()
  })
</script>

<!-- a link the host will not open (it opens http and https only) is not a failure of the app -->
<NotebooksPage {notebooks} onlink={(url) => void host.openLink(url).catch(() => {})} />
