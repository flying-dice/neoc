<script lang="ts">
  import { api } from '@notes/rpc'
  import { host } from '@vos/host'
  import { Notes } from './lib/notes.svelte'
  import NotesPage from './pages/NotesPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const notes = new Notes(api, host)
</script>

<NotesPage {notes} />
