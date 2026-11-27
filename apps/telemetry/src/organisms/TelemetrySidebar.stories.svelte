<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import TelemetrySidebar from './TelemetrySidebar.svelte'
  import type { Probe, View } from '../lib/telemetry.svelte'

  interface Args {
    selected?: View
    count?: number
    probing: Probe | null
    onselect: (view: View) => void
    onprobe: (probe: Probe) => void
  }

  const onselect = fn<(view: View) => void>()
  const onprobe = fn<(probe: Probe) => void>()

  /** What to look at, and the probes that make a trace to look at. */
  const { Story } = defineMeta({
    title: 'Organisms/TelemetrySidebar',
    component: TelemetrySidebar,
    tags: ['autodocs'],
    args: { selected: 'traces', count: 12, probing: null, onselect, onprobe },
    beforeEach: () => {
      onselect.mockClear()
      onprobe.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:260px;height:520px;background:var(--bg-elevated);color:var(--label);font:var(--type-body)">
    <TelemetrySidebar {...args} />
  </div>
{/snippet}

<Story
  name="Traces"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('telemetry-view-traces')).toHaveAttribute('selected')
    await userEvent.click(canvas.getByTestId('telemetry-view-logs'))
    await expect(onselect).toHaveBeenCalledWith('logs')
    await userEvent.click(canvas.getByTestId('telemetry-probe-render'))
    await expect(onprobe).toHaveBeenCalledWith('render')
  }}
/>

<Story name="Logs" args={{ selected: 'logs' }} />

<!-- Before the first read there is no count. -->
<Story name="Not read yet" args={{ count: undefined }} />

<!-- While a probe is on its way, none can be sent. -->
<Story
  name="Sending a probe"
  args={{ probing: 'fail' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('telemetry-probe-answer')).toHaveAttribute('disabled')
  }}
/>
