<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SpanRow from './SpanRow.svelte'
  import { FAILED_TRACE, RENDER_TRACE, SPANS } from '../lib/fakeApi'
  import { waterfall, type Bar } from '../lib/telemetry'

  interface Args {
    bar: Bar
    selected: boolean
    onpress: () => void
  }

  const onpress = fn<() => void>()
  const bars = waterfall(SPANS.filter((s) => s.traceId === RENDER_TRACE))
  const failed = waterfall(SPANS.filter((s) => s.traceId === FAILED_TRACE))

  /** One span of a waterfall: its name, set in by its depth, its bar, and how long it took. */
  const { Story } = defineMeta({
    title: 'Molecules/SpanRow',
    component: SpanRow,
    tags: ['autodocs'],
    args: { bar: bars[1]!, selected: false, onpress },
    beforeEach: () => onpress.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" selection="single" label="Timeline" style="width:420px">
    <ui-section header="Timeline"><SpanRow {...args} /></ui-section>
  </ui-list>
{/snippet}

<Story
  name="The host's span"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-span-2222222222222222'))
    await expect(onpress).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="The page's span, the whole trace" args={{ bar: bars[0]! }} />

<Story name="A backend span, two levels in" args={{ bar: bars[2]! }} />

<Story name="Failed" args={{ bar: failed[1]! }} />

<Story name="Picked" args={{ selected: true }} />
