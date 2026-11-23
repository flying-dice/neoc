<!-- @page SitesPage — the whole app: the web server at the top, and under it the sites it serves, its settings or its log; or what to do when Docker cannot be reached; driven by one window's Pages state -->
<script lang="ts">
  import { ContainerLog, EngineDown } from '@vos/docker'
  import ServerHeader from '../organisms/ServerHeader.svelte'
  import SettingsForm from '../organisms/SettingsForm.svelte'
  import SiteList from '../organisms/SiteList.svelte'
  import { machineOf } from '../lib/format'
  import type { Pages, Tab } from '../lib/pages.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    pages: Pages
    /** The name this window's machine is reached by: what a site's own port is shown and opened at. */
    here?: string
    /** How often the log that is shown is read again while the server runs, in milliseconds. */
    logEvery?: number
  }

  let { pages, here = location.hostname || 'localhost', logEvery = 2000 }: Props = $props()

  const TABS: { value: Tab; label: string }[] = [
    { value: 'sites', label: 'Sites' },
    { value: 'settings', label: 'Settings' },
    { value: 'log', label: 'Log' }
  ]

  const machine = $derived(machineOf(pages.engine.host, here))

  $effect(() => {
    void pages.load()
    return pages.listen()
  })

  // Docker tells the app when the container changes, not when it writes: the log of a running server is read again
  $effect(() => {
    if (!pages.following) return
    const timer = setInterval(() => void pages.refreshLog(), logEvery)
    return () => clearInterval(timer)
  })
</script>

<main class="app" data-testid="pages-app">
  {#if !pages.loaded}
    <div class="waiting">
      {#if !pages.failure}<ui-spinner size="large" label="Asking Docker" data-testid="pages-loading"></ui-spinner>{/if}
    </div>
  {:else if !pages.engine.reachable}
    <div class="rest">
      <EngineDown
        engine={pages.engine}
        draft={pages.hostDraft}
        ondraft={(address) => (pages.hostDraft = address)}
        onconnect={() => void pages.connect()}
        testid="pages"
      />
    </div>
  {:else}
    <ServerHeader server={pages.server} sites={pages.sites} working={pages.working} onapply={() => void pages.apply()} />
    <div class="tabs">
      <ui-segmented
        block
        hit
        label="What is shown"
        options={TABS}
        value={pages.tab}
        onchange={(e) => void pages.show(e.detail.value as Tab)}
        data-testid="pages-tab"
      ></ui-segmented>
    </div>
    <div class="rest">
      {#if pages.tab === 'sites'}
        <SiteList
          sites={pages.sites}
          settings={pages.settings}
          {machine}
          editing={pages.editing}
          working={pages.working}
          busy={(site) => pages.busy(site)}
          onadd={() => pages.startAdding()}
          onedit={(site) => pages.startEditing(site)}
          ondraft={(change) => pages.editSite(change)}
          onsave={() => void pages.saveSite()}
          oncancel={() => pages.cancelEditing()}
          onenable={(site, enabled) => void pages.setSiteEnabled(site, enabled)}
          onremove={(site) => void pages.removeSite(site)}
          onopen={(url) => void pages.open(url)}
        />
      {:else if pages.tab === 'settings'}
        <SettingsForm
          port={pages.portDraft}
          changed={pages.settingsChanged}
          working={pages.working}
          engine={pages.engine}
          server={pages.server}
          hostDraft={pages.hostDraft}
          onport={(typed) => pages.editPort(typed)}
          onsave={() => void pages.saveSettings()}
          onreset={() => pages.resetSettings()}
          onhostdraft={(address) => (pages.hostDraft = address)}
          onconnect={() => void pages.connect()}
        />
      {:else}
        <ContainerLog
          absent={pages.server.state === 'absent'}
          none="The server has no container. It writes here once a site is on."
          logs={pages.logs}
          testid="pages"
        />
      {/if}
    </div>
  {/if}
  <!-- why the last call failed (the backend's own sentence, or the host's), whatever is in front -->
  {#if pages.failure}
    <ui-banner tone="error" data-testid="pages-failure">{pages.failure}</ui-banner>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .tabs {
    padding: 4px 16px 8px;
  }
  .rest {
    flex: 1;
    min-height: 0;
  }
  .waiting {
    flex: 1;
    display: grid;
    place-items: center;
  }
</style>
