<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import TraceList from './TraceList.svelte'
  import { FAILED_TRACE, RENDER_TRACE, T0 } from '../lib/fakeApi'
  import type { TraceSummary } from '../lib/telemetry'

  interface Args {
    traces: readonly TraceSummary[]
    total: number
    query: string
    selected: string | null
    back: boolean
    onsearch: (text: string) => void
    onopen: (trace: TraceSummary) => void
    onback: () => void
  }

  const onsearch = fn<(text: string) => void>()
  const onopen = fn<(trace: TraceSummary) => void>()
  const onback = fn<() => void>()
  const TRACES: TraceSummary[] = [
    { traceId: FAILED_TRACE, name: 'rpc restart', app: 'services', startMs: T0 + 5000, durationMs: 11, spans: 2, errors: 3 },
    { traceId: RENDER_TRACE, name: 'rpc overview', app: 'services', startMs: T0, durationMs: 42, spans: 4, errors: 1 }
  ]

  /** The traces that were recorded, newest first. */
  const { Story } = defineMeta({
    title: 'Organisms/TraceList',
    component: TraceList,
    tags: ['autodocs'],
    args: { traces: TRACES, total: 2, query: '', selected: RENDER_TRACE, back: false, onsearch, onopen, onback },
    beforeEach: () => {
      onsearch.mockClear()
      onopen.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:360px;height:520px;color:var(--label);font:var(--type-body)"><TraceList {...args} /></div>
{/snippet}

<Story
  name="Two traces"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId(`telemetry-trace-${FAILED_TRACE}`))
    await expect(onopen).toHaveBeenCalledWith(TRACES[0])
  }}
/>

<Story name="Nothing recorded yet" args={{ traces: [], total: 0, selected: null }} />

<Story name="Nothing matches" args={{ traces: [], query: 'dns' }} />

<!-- A narrow window shows one column: the way back to the views. -->
<Story
  name="Narrow"
  args={{ back: true, selected: null }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-traces-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
