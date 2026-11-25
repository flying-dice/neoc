<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Overview } from 'app:rpc'
  import OverviewPane from './OverviewPane.svelte'
  import { FIRST_READING, OVERVIEW } from './storyData'

  interface Args {
    overview: Overview
    back: boolean
    onback: () => void
  }

  const onback = fn<() => void>()

  /** The machine as a whole: each core, memory, the disks, the network. */
  const { Story } = defineMeta({
    title: 'Organisms/OverviewPane',
    component: OverviewPane,
    tags: ['autodocs'],
    args: { overview: OVERVIEW, back: false, onback },
    beforeEach: () => onback.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:760px;height:700px;background:var(--bg-grouped-elevated);color:var(--label);font:var(--type-body)">
    <OverviewPane {...args} />
  </div>
{/snippet}

<Story
  name="A busy machine"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-disk-read')).toHaveProperty('value', '14 MB/s')
    await expect(canvas.getByTestId('processes-uptime')).toHaveProperty('value', '3 d 4 h')
  }}
/>

<Story name="The first reading" args={{ overview: FIRST_READING }} />

<!-- Windows has no load average; a machine with no network says so. -->
<Story name="Nothing to report" args={{ overview: { ...OVERVIEW, cpu: { ...OVERVIEW.cpu, loadAverage: undefined }, networks: [], unsupported: ['loadAverage'] } }} />
