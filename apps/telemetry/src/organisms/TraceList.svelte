<!-- @organism TraceList — the traces that were recorded, newest first: what to look for, and a row for each, the one open beside the list marked -->
<script lang="ts">
  import TraceRow from '../molecules/TraceRow.svelte'
  import type { TraceSummary } from '../lib/telemetry'

  interface Props {
    traces: readonly TraceSummary[]
    /** How many there are, before the search. */
    total: number
    query: string
    /** The trace open beside the list; nothing is marked when left out (a narrow window shows one thing). */
    selected?: string | null
    /** Show the way back to the views (a window too narrow for both). */
    back?: boolean
    onsearch: (text: string) => void
    onopen: (trace: TraceSummary) => void
    onback?: () => void
  }

  let { traces, total, query, selected = null, back = false, onsearch, onopen, onback }: Props = $props()

  const valueOf = (e: Event): string => (e.target as HTMLInputElement).value
  const header = $derived(query.trim() === '' ? `${total} ${total === 1 ? 'Trace' : 'Traces'}` : `${traces.length} of ${total}`)
</script>

<section class="list" aria-label="Traces">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Telemetry" onclick={onback} data-testid="telemetry-traces-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Telemetry
      </ui-button>
    {/if}
    <h1 class="ios-large-title" data-testid="telemetry-traces-title">Traces</h1>
  </header>
  <div class="scroll">
    <div class="controls">
      <ui-text-field
        type="search"
        variant="field"
        placeholder="Search by call or app"
        label="Search traces"
        inputmode="search"
        enterkeyhint="search"
        autocapitalize="off"
        autocomplete="off"
        value={query}
        oninput={(e) => onsearch(valueOf(e))}
        data-testid="telemetry-traces-search"
      ></ui-text-field>
    </div>
    {#if traces.length > 0}
      <ui-list variant="insetGrouped" selection="single" label="Traces" data-testid="telemetry-traces">
        <ui-section {header} footer="Kept on this host, in memory, until it restarts.">
          {#each traces as trace (trace.traceId)}
            <TraceRow {trace} selected={trace.traceId === selected} onpress={() => onopen(trace)} />
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state
        icon={total === 0 ? 'network' : 'magnifyingglass'}
        heading={total === 0 ? 'No Traces Yet' : 'No Matching Traces'}
        message={total === 0 ? 'Use an app, or send a probe from the sidebar.' : `Nothing matches “${query.trim()}”.`}
        data-testid="telemetry-traces-empty"
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
    gap: 10px;
    padding: 0 16px 8px;
  }
</style>
