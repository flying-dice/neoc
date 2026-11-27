<script lang="ts">
  import { Keychain, type KeychainEvents, type KeychainRpc } from './lib/keychain.svelte'
  import KeychainPage from './pages/KeychainPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: KeychainRpc = {
    overview: () => RPC.send_request('keychain', '_overview'),
    add: (params) => RPC.send_request('keychain', '_add', params),
    reveal: (params) => RPC.send_request('keychain', '_reveal', params),
    remove: (params) => RPC.send_request('keychain', '_remove', params)
  }
  const events: KeychainEvents = {
    on: (name, fn) => (name === 'changed' ? RPC.on_notification('keychain', 'changed', fn) : vos.host.on(name, fn))
  }
  const keychain = new Keychain(rpc, events)
</script>

<KeychainPage {keychain} />
