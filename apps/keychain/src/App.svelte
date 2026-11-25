<script lang="ts">
  import { api } from '@keychain/rpc'
  import { host } from '@vos/host'
  import { Keychain } from './lib/keychain.svelte'
  import KeychainPage from './pages/KeychainPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const keychain = new Keychain(api, host)
</script>

<KeychainPage {keychain} />
