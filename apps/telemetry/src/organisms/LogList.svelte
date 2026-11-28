<!-- @organism LogList — every app's logs, newest first: what to look for, how severe a log must be to be shown, and a row for each, the one open beside the list marked -->
<script lang="ts">
  import LogRow from '../molecules/LogRow.svelte'
  import { logKey, type Level, type Log } from '../lib/telemetry'
  import type { SourceFilter } from '../lib/telemetry.svelte'

  interface Props {
    logs: readonly Log[]
    query: string
    level: Level
    /** Whose logs are shown: every source's, or one source's. */
    source?: SourceFilter
    /** The log open beside the list (its `logKey`); nothing is marked when left out. */
    selected?: string | null
    back?: boolean
    onsearch: (text: string) => void
    onlevel: (level: Level) => void
    onsource?: (source: SourceFilter) => void
    onopen: (log: Log) => void
    onback?: () => void
  }

  let { logs, query, level, source = 'all', selected = null, back = false, onsearch, onlevel, onsource, onopen, onback }: Props = $props()
  const SOURCES = [
    { value: 'all', label: 'Everywhere' },
    { value: 'browser', label: 'Page' },
    { value: 'backend', label: 'Backend' },
    { value: 'host', label: 'Host' }
  ]

  const LEVELS = [
    { value: 'debug', label: 'All' },
    { value: 'info', label: 'Info' },
    { value: 'warn', label: 'Warnings' },
    { value: 'error', label: 'Errors' }
  ]
  const valueOf = (e: Event): string => (e.target as HTMLInputElement).value
</script>

<section class="list" aria-label="Logs">
  <header class="bar">
    {#if back}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Telemetry" onclick={onback} data-testid="telemetry-logs-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Telemetry
      </ui-button>
    {/if}
    <h1 class="ios-large-title" data-testid="telemetry-logs-title">Logs</h1>
  </header>
  <div class="scroll">
    <div class="controls">
      <ui-text-field
        type="search"
        variant="field"
        placeholder="Search by message or app"
        label="Search logs"
        inputmode="search"
        enterkeyhint="search"
        autocapitalize="off"
        autocomplete="off"
        value={query}
        oninput={(e) => onsearch(valueOf(e))}
        data-testid="telemetry-logs-search"
      ></ui-text-field>
      <ui-segmented block label="Show" value={level} options={LEVELS} onchange={(e) => onlevel(e.detail.value as Level)} data-testid="telemetry-logs-level"
      ></ui-segmented>
      <ui-segmented block label="Written by" value={source} options={SOURCES} onchange={(e) => onsource?.(e.detail.value as SourceFilter)} data-testid="telemetry-logs-source"
      ></ui-segmented>
    </div>
    {#if logs.length > 0}
      <ui-list variant="insetGrouped" selection="single" label="Logs" data-testid="telemetry-logs">
        <ui-section header="{logs.length} {logs.length === 1 ? 'Log' : 'Logs'}">
          {#each logs as log, i (`${logKey(log)}:${i}`)}
            <LogRow {log} showApp selected={logKey(log) === selected} onpress={() => onopen(log)} testid="telemetry-log-{i}" />
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state
        icon={query.trim() === '' ? 'doc' : 'magnifyingglass'}
        heading={query.trim() === '' ? 'No Logs' : 'No Matching Logs'}
        message={query.trim() === '' ? 'Nothing was logged at this level yet.' : `Nothing matches “${query.trim()}”.`}
        data-testid="telemetry-logs-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .list {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
  }
  .bar {
    flex: none;
    padding: 4px 16px 8px 20px;
  }
  .bar h1 {
    margin: 0;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .controls {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 16px 8px;
  }
</style>
