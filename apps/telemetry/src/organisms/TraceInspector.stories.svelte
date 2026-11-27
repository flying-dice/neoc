<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import TraceInspector from './TraceInspector.svelte'
  import { FAILED_TRACE, LOGS, RENDER_TRACE, SPANS, T0 } from '../lib/fakeApi'
  import { logKey, waterfall, type Bar, type Log, type Span, type TraceSummary } from '../lib/telemetry'
  import type { Choice } from '../lib/telemetry.svelte'

  interface Args {
    traceId: string
    summary: TraceSummary | null
    bars: readonly Bar[]
    logs: readonly Log[]
    choice: Choice
    span: Span | null
    log: Log | null
    back: boolean
    onback: () => void
    onchoose: (choice: NonNullable<Choice>) => void
  }

  const onchoose = fn<(choice: NonNullable<Choice>) => void>()
  const onback = fn<() => void>()
  const of = (traceId: string) => ({
    bars: waterfall(SPANS.filter((s) => s.traceId === traceId)),
    logs: LOGS.filter((l) => l.traceId === traceId)
  })
  const render = of(RENDER_TRACE)
  const failed = of(FAILED_TRACE)
  const renderSummary: TraceSummary = { traceId: RENDER_TRACE, name: 'rpc overview', app: 'services', startMs: T0, durationMs: 42, spans: 4, errors: 1 }

  /** One trace in full: its waterfall, its logs, and what was picked in it. */
  const { Story } = defineMeta({
    title: 'Organisms/TraceInspector',
    component: TraceInspector,
    tags: ['autodocs'],
    args: { traceId: RENDER_TRACE, summary: renderSummary, ...render, choice: null, span: null, log: null, back: false, onback, onchoose },
    beforeEach: () => {
      onchoose.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:640px;color:var(--label);font:var(--type-body)"><TraceInspector {...args} /></div>
{/snippet}

<!-- The Services window of NEOC-520: the call answered, then the page failed to draw what it returned. -->
<Story
  name="A render that failed after the call"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('telemetry-trace-failed')).toBeVisible()
    await userEvent.click(canvas.getByTestId('telemetry-span-2222222222222222'))
    await expect(onchoose).toHaveBeenCalledWith({ kind: 'span', id: '2222222222222222' })
  }}
/>

<Story name="A span picked" args={{ choice: { kind: 'span', id: '2222222222222222' }, span: SPANS[1]! }} />

<!-- Picking a log shows it in full and the span it was written in. -->
<Story name="A log picked" args={{ choice: { kind: 'log', key: logKey(LOGS[1]!) }, log: LOGS[1]!, span: SPANS[0]! }} />

<Story
  name="A call that failed"
  args={{ traceId: FAILED_TRACE, summary: { traceId: FAILED_TRACE, name: 'rpc restart', app: 'services', startMs: T0 + 5000, durationMs: 11, spans: 2, errors: 3 }, ...failed }}
/>

<!-- Opened from a log before the trace was read. -->
<Story name="Reading the trace" args={{ summary: null, bars: [], logs: [] }} />

<Story name="Narrow" args={{ back: true }} />
