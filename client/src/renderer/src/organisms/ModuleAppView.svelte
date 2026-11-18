<!--
  @organism ModuleAppView — mounts an installed app module's client inside a
  window, providing its scoped SDK API. Loads the client bundle lazily.
-->
<script lang="ts">
  import { provideAppApi } from '@agentos/sdk/app/client'
  import { ActivityIndicator, Text } from '@agentos/ui/atoms'
  import { appRuntime } from '../lib/appRuntime'
  import { moduleApp } from '../lib/apps.svelte'
  import { loadClientApp } from '../lib/appLoaders'

  interface Props {
    appId: string
  }

  let { appId }: Props = $props()

  // A window never changes app: read the id once for the window's lifetime.
  // svelte-ignore state_referenced_locally
  const id = appId
  provideAppApi(appRuntime.apiFor(id))
  const installed = moduleApp(id)
  const loading = installed
    ? installed.status === 'failed'
      ? Promise.reject(new Error(installed.error ?? `${installed.manifest.name} failed to start`))
      : loadClientApp(installed)
    : Promise.reject(new Error(`App "${id}" is not installed`))
</script>

{#await loading}
  <div class="state"><ActivityIndicator size="large" /></div>
{:then module}
  {@const App = module.component}
  <App />
{:catch error}
  <div class="state">
    <Text style="headline">Couldn’t open app</Text>
    <Text style="footnote" color="secondary">{error.message}</Text>
  </div>
{/await}

<style>
  .state {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 24px;
    text-align: center;
  }
</style>
