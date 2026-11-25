<!-- @molecule TraceRow — one trace in the list: the call it started with, when, its app and how many spans, and how long it took, marked when something in it failed -->
<script lang="ts">
  import { clock, duration, type TraceSummary } from '../lib/telemetry'

  interface Props {
    trace: TraceSummary
    /** It is the trace that is open beside the list. */
    selected?: boolean
    onpress: () => void
  }

  let { trace, selected = false, onpress }: Props = $props()

  const failed = $derived(trace.errors > 0)
  const sub = $derived(
    `${clock(trace.startMs)} · ${trace.app ?? 'No app'} · ${trace.spans} ${trace.spans === 1 ? 'span' : 'spans'}${failed ? ` · ${trace.errors} ${trace.errors === 1 ? 'error' : 'errors'}` : ''}`
  )
  const took = $derived(duration(trace.durationMs))
</script>

<ui-row
  heading={trace.name}
  subheading={sub}
  icon={failed ? 'exclamationmark.triangle' : 'network'}
  icon-color={failed ? 'red' : 'teal'}
  value={took}
  label="{trace.name} {sub} {took}"
  {selected}
  {onpress}
  data-testid="telemetry-trace-{trace.traceId}"
></ui-row>
