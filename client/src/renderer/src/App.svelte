<!--
  Composition root — the only component allowed outside an atomic layer.
  It applies system settings and maps app ids to built-in pages or to
  installed app modules.
-->
<script lang="ts">
  import DesktopPage from './pages/DesktopPage.svelte'
  import DiscoverPage from './pages/DiscoverPage.svelte'
  import AgentsPage from './pages/AgentsPage.svelte'
  import SettingsPage from './pages/SettingsPage.svelte'
  import FilesPage from './pages/FilesPage.svelte'
  import GalleryPage from './pages/GalleryPage.svelte'
  import TerminalPage from './pages/TerminalPage.svelte'
  import ModuleAppView from './organisms/ModuleAppView.svelte'
  import { settings } from './lib/settings.svelte'
  import { windowManager } from './lib/windowManager.svelte'
  import { moduleApp, onAppsRemoved, type AppId } from './lib/apps.svelte'
  import { hostBridge } from '@agentos/sdk/host/client'

  $effect(() => settings.apply(document.documentElement))

  // App modules can ask the host to open their window (e.g. from an agent tool).
  // Opened on the user's behalf, so they join the current stage beside the requester.
  $effect(() =>
    hostBridge()?.apps.onOpenRequest((appId) => windowManager.open(appId, { alongside: true }))
  )

  // An app uninstalled (here or from another client) closes its windows first.
  $effect(() =>
    onAppsRemoved((ids) => {
      for (const win of [...windowManager.windows])
        if (ids.includes(win.appId)) windowManager.close(win.id)
    })
  )

  // Land on the Discover window.
  windowManager.open('discover')
</script>

{#snippet windowContent(appId: AppId)}
  {#if appId === 'discover'}
    <DiscoverPage />
  {:else if appId === 'agents'}
    <AgentsPage />
  {:else if appId === 'settings'}
    <SettingsPage />
  {:else if appId === 'files'}
    <FilesPage />
  {:else if appId === 'gallery'}
    <GalleryPage />
  {:else if appId === 'terminal'}
    <TerminalPage />
  {:else if moduleApp(appId)}
    <ModuleAppView {appId} />
  {/if}
{/snippet}

<DesktopPage {windowContent} />
