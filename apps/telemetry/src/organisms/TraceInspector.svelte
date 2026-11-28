<!-- @organism TraceInspector — one trace in full: the call it started with and how it went, its spans as a waterfall from the page to the host and the backend, its logs, and the span or log that was picked, in rows -->
<script lang="ts">
  import LogDetails from '../molecules/LogDetails.svelte'
  import LogRow from '../molecules/LogRow.svelte'
  import SpanDetails from '../molecules/SpanDetails.svelte'
  import SpanRow from '../molecules/SpanRow.svelte'
  import { SOURCE_LABEL, SOURCE_TINT, clock, duration, logKey, type Bar, type Log, type Source, type Span, type TraceSummary } from '../lib/telemetry'
  import type { Choice } from '../lib/telemetry.svelte'

  interface Props {
    traceId: string
    /** The trace's summary, while it is still in the list. */
    summary?: TraceSummary | null
    bars: readonly Bar[]
    logs: readonly Log[]
    choice?: Choice
    /** The span to show in full: the one picked, or the one the picked log was written in. */
    span?: Span | null
    /** The log to show in full. */
    log?: Log | null
    back?: boolean
    onback?: () => void
    onchoose: (choice: NonNullable<Choice>) => void
  }

  let { traceId, summary = null, bars, logs, choice = null, span = null, log = null, back = false, onback, onchoose }: Props = $props()

  const root = $derived(bars[0]?.span ?? null)
  const failed = $derived((summary?.errors ?? 0) > 0)
  const SOURCES: Source[] = ['browser', 'host', 'backend']
  const subline = $derived(
    summary ? `${summary.app ?? 'No app'} · ${duration(summary.durationMs)} · ${summary.spans} ${summary.spans === 1 ? 'span' : 'spans'} · ${clock(summary.startMs)}` : ''
  )
</script>

<section class="inspector" aria-label="Trace {root?.name ?? traceId}">
  {#if back}
    <header class="bar">
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the traces" onclick={onback} data-testid="telemetry-trace-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Traces
      </ui-button>
    </header>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroll" tabindex="0" role="region" aria-label="Details of the trace">
    <div class="head">
      <h1 class="ios-large-title" data-testid="telemetry-trace-name">{summary?.name ?? root?.name ?? 'Trace'}</h1>
      {#if subline}<p class="ios-subheadline ios-secondary" data-testid="telemetry-trace-summary">{subline}</p>{/if}
      <p class="ios-footnote ios-secondary mono" data-testid="telemetry-trace-id">{traceId}</p>
      {#if failed}
        <ui-banner tone="error" data-testid="telemetry-trace-failed">
          {summary?.errors === 1 ? 'Something in this trace failed.' : `${summary?.errors} things in this trace failed.`} The failed spans and the error logs are in red.
        </ui-banner>
      {/if}
      <p class="legend ios-footnote ios-secondary" aria-hidden="true">
        {#each SOURCES as source (source)}<span><i style:background={SOURCE_TINT[source]}></i>{SOURCE_LABEL[source]}</span>{/each}
      </p>
    </div>

    {#if bars.length === 0}
      <div class="waiting"><ui-spinner size="large" label="Reading the trace" data-testid="telemetry-trace-loading"></ui-spinner></div>
    {:else}
      <ui-list variant="insetGrouped" selection="single" label="Timeline" data-testid="telemetry-trace">
        <ui-section header="Timeline" footer="Pick a span or a log to see it in full.">
          {#each bars as bar (bar.span.spanId)}
            <SpanRow {bar} selected={choice?.kind === 'span' && choice.id === bar.span.spanId} onpress={() => onchoose({ kind: 'span', id: bar.span.spanId })} />
          {/each}
        </ui-section>
      </ui-list>
      {#if span}<ui-list variant="insetGrouped" label="Span"><SpanDetails {span} /></ui-list>{/if}
      <ui-list variant="insetGrouped" selection="single" label="Logs" data-testid="telemetry-trace-logs">
        <ui-section header="Logs" footer={logs.length === 0 ? 'Nothing was logged in this trace.' : undefined}>
          {#each logs as l, i (`${logKey(l)}:${i}`)}
            <LogRow
              log={l}
              selected={choice?.kind === 'log' && choice.key === logKey(l)}
              onpress={() => onchoose({ kind: 'log', key: logKey(l) })}
              testid="telemetry-trace-log-{i}"
            />
          {/each}
        </ui-section>
      </ui-list>
      {#if log}<ui-list variant="insetGrouped" label="Log"><LogDetails {log} header="Log" /></ui-list>{/if}
    {/if}
  </div>
</section>

<style>
  .inspector {
    box-sizing: border-box;
    height: 100%;
    min-width: 0;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
  }
  .bar {
    flex: none;
    padding: 0 16px 4px 12px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding-bottom: 24px;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 20px 8px;
  }
  .head h1,
  .head p {
    margin: 0;
  }
  .mono {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
  .legend {
    display: flex;
    gap: 16px;
    padding-top: 8px;
  }
  .legend i {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 4px;
    border-radius: 50%;
  }
  .waiting {
    display: grid;
    place-items: center;
    padding: 48px 16px;
  }
</style>
