<script lang="ts">
  import { Notes, type NotesEvents, type NotesRpc } from './lib/notes.svelte'
  import NotesPage from './pages/NotesPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: NotesRpc = {
    board: () => RPC.send_request('notes', '_board'),
    addFolder: (params) => RPC.send_request('notes', '_add_folder', params),
    removeFolder: (params) => RPC.send_request('notes', '_remove_folder', params),
    create: (params) => RPC.send_request('notes', '_create', params),
    read: (params) => RPC.send_request('notes', '_read', params),
    write: (params) => RPC.send_request('notes', '_write', params),
    pin: (params) => RPC.send_request('notes', '_pin', params),
    move: (params) => RPC.send_request('notes', '_move', params),
    remove: (params) => RPC.send_request('notes', '_remove', params),
    restore: (params) => RPC.send_request('notes', '_restore', params),
    purge: (params) => RPC.send_request('notes', '_purge', params),
    search: (params) => RPC.send_request('notes', '_search', params)
  }
  const events: NotesEvents = {
    on: (name, fn) => (name === 'notes_changed' ? RPC.on_notification('notes', 'notes_changed', fn) : vos.host.on(name, fn))
  }
  const notes = new Notes(rpc, events)
</script>

<NotesPage {notes} />
