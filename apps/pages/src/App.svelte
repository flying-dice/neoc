<script lang="ts">
  import { Pages, type PagesHost, type PagesRpc } from './lib/pages.svelte'
  import SitesPage from './pages/SitesPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: PagesRpc = {
    overview: () => RPC.send_request('pages', '_overview'),
    connect: (params) => RPC.send_request('pages', '_connect', params),
    apply: () => RPC.send_request('pages', '_apply'),
    saveSettings: (params) => RPC.send_request('pages', '_save_settings', params),
    addSite: (params) => RPC.send_request('pages', '_create_site', params),
    updateSite: (params) => RPC.send_request('pages', '_update_site', params),
    setSiteEnabled: (params) => RPC.send_request('pages', '_set_site_enabled', params),
    removeSite: (params) => RPC.send_request('pages', '_delete_site', params),
    logs: (params) => RPC.send_request('pages', '_logs', params)
  }
  const host: PagesHost = {
    on: (name, fn) => (name === 'changed' ? RPC.on_notification('pages', 'changed', fn) : vos.host.on(name, fn)),
    open_link: (url) => vos.host.open_link(url)
  }
  const pages = new Pages(rpc, host)
</script>

<SitesPage {pages} />
