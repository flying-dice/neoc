<!--
  Composition root — the only component allowed outside an atomic layer.
  It gates the shell on pairing, applies system settings, and maps app ids to built-in pages or, for an installed
  app, to the frame its page runs in.
-->
<script lang="ts">
  import DesktopPage from './pages/DesktopPage.svelte'
  import DiscoverPage from './pages/DiscoverPage.svelte'
  import SettingsPage from './pages/SettingsPage.svelte'
  import FilesPage from './pages/FilesPage.svelte'
  import PairingPage from './pages/PairingPage.svelte'
  import AppFrame from './organisms/AppFrame.svelte'
  import { settings } from './lib/settings.svelte'
  import { host } from './lib/host.svelte'
  import { windowManager, type WindowState } from './lib/windowManager.svelte'
  import { installedApp, onAppsRemoved } from './lib/apps.svelte'

  $effect(() => settings.apply(document.documentElement))

  // The host asks for a window (an agent's tool bringing an app forward): open it beside what the person is
  // looking at, or bring the app's own window forward.
  $effect(() =>
    host.onWindowRequest(({ app }) => {
      windowManager.open(app, { alongside: true }) // ignores an app that is not running
    })
  )

  // An app that stops running (uninstalled, disabled or failed, here or from another client) loses its windows.
  $effect(() =>
    onAppsRemoved((ids) => {
      for (const win of [...windowManager.windows])
        if (ids.includes(win.appId)) windowManager.close(win.id)
    })
  )

  // Land on the Discover window, once the desktop is up.
  let landed = false
  $effect(() => {
    if (host.phase === 'ready' && !landed) {
      landed = true
      windowManager.open('discover')
    }
  })
</script>

{#snippet windowContent(win: WindowState)}
  {#if win.appId === 'discover'}
    <DiscoverPage />
  {:else if win.appId === 'settings'}
    <SettingsPage />
  {:else if win.appId === 'files'}
    <FilesPage />
  {:else}
    <AppFrame
      appId={win.appId}
      name={installedApp(win.appId)?.name ?? win.appId}
      windowId={win.id}
      permissions={installedApp(win.appId)?.permissions ?? []}
    />
  {/if}
{/snippet}

{#if host.phase === 'ready' || host.phase === 'connecting'}
  <DesktopPage {windowContent} />
{:else if host.phase === 'checking'}
  <main class="splash" data-testid="splash" aria-busy="true" aria-label="Starting vos"></main>
{:else}
  <PairingPage
    onpair={(code, name) => host.pair(code, name)}
    unreachable={host.phase === 'unreachable' ? host.error : null}
    onretry={() => host.retry()}
  />
{/if}

<style>
  .splash {
    min-height: 100vh;
    background: var(--wallpaper-default, var(--bg-grouped));
  }
</style>
