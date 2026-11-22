<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Terminal } from './lib/terminal.svelte'
  import TerminalPage from './pages/TerminalPage.svelte'

  // The composition root: the one place the state meets the real backend and the host's events.
  const terminal = new Terminal(api, host)

  // The window is closing: end the shell. (A page that goes away without being asked, a reload or a closed browser,
  // cannot be counted on to say so: the backend ends a session it stops hearing from.)
  host.onTeardown(() => terminal.close())
</script>

<svelte:window onpagehide={() => void terminal.close()} />

<TerminalPage {terminal} />
