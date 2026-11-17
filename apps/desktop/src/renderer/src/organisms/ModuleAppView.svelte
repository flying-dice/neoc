<!--
  @organism ModuleAppView — mounts an installed app module's renderer inside a
  window, providing its scoped SDK API. Loads the module bundle lazily.
-->
<script lang="ts">
  import { provideAppApi } from '@agentos/sdk/app/renderer'
  import { ActivityIndicator, Text } from '@agentos/ui/atoms'
  import { appRuntime } from '../lib/appRuntime'
  import { moduleApp } from '../lib/installedApps'

  interface Props {
    appId: string
  }

  let { appId }: Props = $props()

  // Context is fixed for the window's lifetime; an app view never changes app.
  // svelte-ignore state_referenced_locally
  provideAppApi(appRuntime.apiFor(appId))
  // svelte-ignore state_referenced_locally
  const loading =
    moduleApp(appId)?.load() ?? Promise.reject(new Error(`App "${appId}" is not installed`))
</script>

{#await loading}
  <div class="state"><ActivityIndicator size="large" /></div>
{:then module}
  {@const App = module.default.component}
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
