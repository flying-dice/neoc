<script lang="ts">
  import { Notebooks, type NotebooksEvents, type NotebooksRpc } from './lib/notebooks.svelte'
  import NotebooksPage from './pages/NotebooksPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: NotebooksRpc = {
    library: () => RPC.send_request('notebooks', '_library'),
    read: (params) => RPC.send_request('notebooks', '_read', params),
    create: (params) => RPC.send_request('notebooks', '_create', params),
    follow: (params) => RPC.send_request('notebooks', '_follow', params),
    save: (params) => RPC.send_request('notebooks', '_save', params),
    rename: (params) => RPC.send_request('notebooks', '_rename', params),
    pin: (params) => RPC.send_request('notebooks', '_pin', params),
    remove: (params) => RPC.send_request('notebooks', '_remove', params),
    search: (params) => RPC.send_request('notebooks', '_search', params),
    addNotebook: (params) => RPC.send_request('notebooks', '_add_notebook', params),
    removeNotebook: (params) => RPC.send_request('notebooks', '_remove_notebook', params)
  }
  const events: NotebooksEvents = {
    on: (name, fn) => (name === 'notebooks_changed' ? RPC.on_notification('notebooks', 'notebooks_changed', fn) : vos.host.on(name, fn))
  }
  const notebooks = new Notebooks(rpc, events)

  // what was written a moment ago is sent before the host takes the page away
  vos.host.on_teardown(async () => {
    await notebooks.flush()
  })
</script>

<!-- a link the host will not open (it opens http and https only) is not a failure of the app -->
<NotebooksPage {notebooks} onlink={(url) => void vos.host.open_link(url).catch(() => {})} />
