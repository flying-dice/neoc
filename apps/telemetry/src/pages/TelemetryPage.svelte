<!-- @page TelemetryPage — the whole app: the views and the probes beside the traces (or the logs), the trace or log that was picked in the next column; driven by one window's Telemetry state -->
<script lang="ts">
  import { untrack } from 'svelte'
  import LogInspector from '../organisms/LogInspector.svelte'
  import LogList from '../organisms/LogList.svelte'
  import TelemetrySidebar from '../organisms/TelemetrySidebar.svelte'
  import TraceInspector from '../organisms/TraceInspector.svelte'
  import TraceList from '../organisms/TraceList.svelte'
  import { logKey } from '../lib/telemetry'
  import type { Telemetry } from '../lib/telemetry.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real reader and backend, a story with a fake. */
    telemetry: Telemetry
    /** How often what is shown is read again while the window is in front, in milliseconds; 0 never (a story). */
    every?: number
  }

  let { telemetry, every = 3000 }: Props = $props()

  // in a window too narrow for all the columns, the split view shows one: the kit says when, the same for every app
  let narrow = $state(false)
  let showing = $state(!document.hidden)
  const entry = $derived(telemetry.entry)

  $effect(() => {
    untrack(() => void telemetry.refresh())
  })

  // what was recorded is read again while the window is in front; a hidden window reads nothing
  $effect(() => {
    if (every === 0 || !showing) return
    const timer = setInterval(() => void telemetry.refresh(), every)
    return () => clearInterval(timer)
  })

  $effect(() => {
    const changed = (): void => {
      showing = !document.hidden
      if (showing) void telemetry.refresh()
    }
    document.addEventListener('visibilitychange', changed)
    return () => document.removeEventListener('visibilitychange', changed)
  })
</script>

<main class="app" data-testid="telemetry-app">
  <ui-split-view column={telemetry.pane} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <TelemetrySidebar
        selected={narrow ? undefined : telemetry.view}
        count={telemetry.loaded ? telemetry.traceCount : undefined}
        probing={telemetry.probing}
        onselect={(view) => telemetry.show(view)}
        onprobe={(probe) => void telemetry.probe(probe)}
      />
    </div>
    <div slot="content">
      {#if telemetry.view === 'traces'}
        <TraceList
          traces={telemetry.traces}
          total={telemetry.traceCount}
          query={telemetry.query}
          selected={narrow ? null : telemetry.selected}
          back={narrow}
          onsearch={(text) => telemetry.search(text)}
          onopen={(trace) => void telemetry.open(trace.traceId)}
          onback={() => telemetry.back()}
        />
      {:else}
        <LogList
          logs={telemetry.logs}
          query={telemetry.query}
          level={telemetry.level}
          selected={narrow || !entry ? null : logKey(entry)}
          back={narrow}
          onsearch={(text) => telemetry.search(text)}
          onlevel={(level) => telemetry.filter(level)}
          onopen={(log) => telemetry.inspect(log)}
          onback={() => telemetry.back()}
        />
      {/if}
    </div>
    <div class="detail">
      {#if telemetry.unavailable}
        <div class="waiting">
          <ui-empty-state icon="exclamationmark.triangle" heading="Telemetry Unavailable" message={telemetry.unavailable} data-testid="telemetry-unavailable"
          ></ui-empty-state>
        </div>
      {:else if !telemetry.loaded}
        <div class="waiting"><ui-spinner size="large" label="Reading what was recorded" data-testid="telemetry-loading"></ui-spinner></div>
      {:else if telemetry.view === 'traces' && telemetry.selected}
        <TraceInspector
          traceId={telemetry.selected}
          summary={telemetry.summary}
          bars={telemetry.bars}
          logs={telemetry.traceLogs}
          choice={telemetry.choice}
          span={telemetry.pickedSpan}
          log={telemetry.pickedLog}
          back={narrow}
          onback={() => telemetry.back()}
          onchoose={(choice) => telemetry.choose(choice)}
        />
      {:else if telemetry.view === 'logs' && entry}
        <LogInspector log={entry} back={narrow} onback={() => telemetry.back()} ontrace={(traceId) => void telemetry.open(traceId)} />
      {:else}
        <div class="waiting">
          <ui-empty-state
            icon={telemetry.view === 'traces' ? 'network' : 'doc'}
            heading={telemetry.view === 'traces' ? 'No Trace Selected' : 'No Log Selected'}
            message={telemetry.view === 'traces' ? 'Pick a trace to follow its call from the page to the host and the backend.' : 'Pick a log to see its fields and its trace.'}
            data-testid="telemetry-none"
          ></ui-empty-state>
        </div>
      {/if}
    </div>
  </ui-split-view>
  <!-- why the last read failed, whichever column is in front -->
  {#if telemetry.failure}
    <ui-banner tone="error" data-testid="telemetry-failure">{telemetry.failure}</ui-banner>
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
    --ui-split-sidebar-width: 240px;
    --ui-split-content-width: 340px;
  }
  ui-split-view {
    flex: 1;
    min-height: 0;
  }
  .detail {
    height: 100%;
  }
  .waiting {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 16px;
    background: var(--bg-grouped-elevated);
  }
</style>
