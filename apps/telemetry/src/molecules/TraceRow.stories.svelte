<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import TraceRow from './TraceRow.svelte'
  import { FAILED_TRACE, RENDER_TRACE, T0 } from '../lib/fakeApi'
  import type { TraceSummary } from '../lib/telemetry'

  interface Args {
    trace: TraceSummary
    selected: boolean
    onpress: () => void
  }

  const onpress = fn<() => void>()
  const ok: TraceSummary = { traceId: RENDER_TRACE, name: 'rpc overview', app: 'services', startMs: T0, durationMs: 42, spans: 4, errors: 0 }

  /** One trace: the call it started with, its app and spans, and how long it took and when. */
  const { Story } = defineMeta({
    title: 'Molecules/TraceRow',
    component: TraceRow,
    tags: ['autodocs'],
    args: { trace: ok, selected: false, onpress },
    beforeEach: () => onpress.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" selection="single" label="Traces" style="width:360px">
    <ui-section header="Traces"><TraceRow {...args} /></ui-section>
  </ui-list>
{/snippet}

<Story
  name="A call that went well"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId(`telemetry-trace-${RENDER_TRACE}`))
    await expect(onpress).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Open beside the list" args={{ selected: true }} />

<Story name="Something failed" args={{ trace: { traceId: FAILED_TRACE, name: 'rpc restart', app: 'services', startMs: T0 + 5000, durationMs: 11, spans: 2, errors: 3 } }} />

<!-- The longest name the row must survive. -->
<Story
  name="A long call name"
  args={{ trace: { ...ok, name: 'rpc listUnitsWithTheirDependenciesAndTheirTimersAndSockets', app: 'a-very-long-app-identifier', durationMs: 12_345 } }}
/>
