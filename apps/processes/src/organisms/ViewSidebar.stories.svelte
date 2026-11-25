<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ViewSidebar from './ViewSidebar.svelte'
  import type { Interval, View } from '../lib/processes.svelte'

  interface Args {
    selected?: View
    count?: number
    interval: Interval
    updatedAt: number | null
    onselect: (view: View) => void
    oninterval: (interval: Interval) => void
  }

  const onselect = fn<(view: View) => void>()
  const oninterval = fn<(interval: Interval) => void>()

  /** What to look at, and how often the machine is read. */
  const { Story } = defineMeta({
    title: 'Organisms/ViewSidebar',
    component: ViewSidebar,
    tags: ['autodocs'],
    args: { selected: 'processes', count: 412, interval: 2, updatedAt: new Date(2026, 5, 9, 9, 41, 7).getTime(), onselect, oninterval },
    beforeEach: () => {
      onselect.mockClear()
      oninterval.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:240px;height:420px;background:var(--bg-elevated);color:var(--label);font:var(--type-body)">
    <ViewSidebar {...args} />
  </div>
{/snippet}

<Story
  name="Processes"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('processes-view-processes')).toHaveAttribute('selected')
    await userEvent.click(canvas.getByTestId('processes-view-overview'))
    await expect(onselect).toHaveBeenCalledWith('overview')
  }}
/>

<Story name="Overview" args={{ selected: 'overview' }} />

<!-- Before the first reading there is no count and nothing to say of an update. -->
<Story name="Not read yet" args={{ count: undefined, updatedAt: null }} />

<Story name="Paused" args={{ interval: 0 }} />
