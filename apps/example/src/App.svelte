<script lang="ts">
  import { api, host } from 'app:rpc'
  import { Example } from './lib/example.svelte'
  import ExamplePage from './pages/ExamplePage.svelte'

  // The composition root: the one place the state meets the real backend (`api`, generated from openrpc.json) and the
  // host's events (`host`). Everything under it takes the state as a prop, so a story or a test can hand it a fake.
  const example = new Example(api, host)
</script>

<ExamplePage {example} />
