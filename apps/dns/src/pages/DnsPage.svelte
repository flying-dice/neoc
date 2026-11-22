<!-- @page DnsPage — the whole app: the server and its switch at the top, and under it the names it answers for, its settings or its log; or what to do when Docker cannot be reached; driven by one window's Dns state -->
<script lang="ts">
  import EngineDown from '../organisms/EngineDown.svelte'
  import RecordList from '../organisms/RecordList.svelte'
  import ServerHeader from '../organisms/ServerHeader.svelte'
  import ServerLog from '../organisms/ServerLog.svelte'
  import SettingsForm from '../organisms/SettingsForm.svelte'
  import type { Dns, Tab } from '../lib/dns.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    dns: Dns
    /** How often the log that is shown is read again while the server runs, in milliseconds. */
    logEvery?: number
  }

  let { dns, logEvery = 2000 }: Props = $props()

  const TABS: { value: Tab; label: string }[] = [
    { value: 'records', label: 'Names' },
    { value: 'settings', label: 'Settings' },
    { value: 'log', label: 'Log' }
  ]

  $effect(() => {
    void dns.load()
    return dns.listen()
  })

  // Docker tells the app when the container changes, not when it writes: the log of a running server is read again
  $effect(() => {
    if (!dns.following) return
    const timer = setInterval(() => void dns.refreshLog(), logEvery)
    return () => clearInterval(timer)
  })
</script>

<main class="app" data-testid="dns-app">
  {#if !dns.loaded}
    <div class="waiting">
      {#if !dns.failure}<ui-spinner size="large" label="Asking Docker" data-testid="dns-loading"></ui-spinner>{/if}
    </div>
  {:else if !dns.engine.reachable}
    <div class="rest">
      <EngineDown
        engine={dns.engine}
        draft={dns.hostDraft}
        ondraft={(address) => (dns.hostDraft = address)}
        onconnect={() => void dns.connect()}
      />
    </div>
  {:else}
    <ServerHeader
      server={dns.server}
      settings={dns.settings}
      working={dns.working}
      onenable={(enabled) => void dns.setEnabled(enabled)}
      onapply={() => void dns.apply()}
    />
    <div class="tabs">
      <ui-segmented
        block
        hit
        label="What is shown"
        options={TABS}
        value={dns.tab}
        onchange={(e) => void dns.show(e.detail.value as Tab)}
        data-testid="dns-tab"
      ></ui-segmented>
    </div>
    <div class="rest">
      {#if dns.tab === 'records'}
        <RecordList
          records={dns.records}
          draft={dns.record}
          problems={dns.recordProblems}
          refused={dns.recordRefused}
          working={dns.working}
          removing={(record) => dns.removing(record)}
          ondraft={(change) => dns.editRecord(change)}
          onadd={() => void dns.addRecord()}
          onremove={(record) => void dns.removeRecord(record)}
        />
      {:else if dns.tab === 'settings'}
        <SettingsForm
          draft={dns.settingsDraft}
          problems={dns.settingsProblems}
          refused={dns.settingsRefused}
          changed={dns.settingsChanged}
          working={dns.working}
          engine={dns.engine}
          server={dns.server}
          hostDraft={dns.hostDraft}
          ondraft={(change) => dns.editSettings(change)}
          onsave={() => void dns.saveSettings()}
          onreset={() => dns.resetSettings()}
          onhostdraft={(address) => (dns.hostDraft = address)}
          onconnect={() => void dns.connect()}
        />
      {:else}
        <ServerLog server={dns.server} logs={dns.logs} />
      {/if}
    </div>
  {/if}
  <!-- why the last call failed (the host's sentence, or Docker's), whatever is in front; what is wrong with something typed is said at its field -->
  {#if dns.failure}
    <ui-banner tone="error" data-testid="dns-failure">{dns.failure}</ui-banner>
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
