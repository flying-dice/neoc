<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import type { Overview } from '@processes/rpc'
  import UsageTiles from './UsageTiles.svelte'
  import { FIRST_READING, OVERVIEW } from './storyData'

  interface Args {
    overview: Overview
  }

  /** The top of the list: the processor, memory, swap and network. */
  const { Story } = defineMeta({
    title: 'Organisms/UsageTiles',
    component: UsageTiles,
    tags: ['autodocs'],
    args: { overview: OVERVIEW },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;background:var(--bg-grouped-elevated);color:var(--label);font:var(--type-body)">
    <UsageTiles {...args} />
  </div>
{/snippet}

<Story
  name="A busy machine"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-tile-cpu-value')).toHaveTextContent('34%')
    await expect(canvas.getByTestId('processes-tile-memory-value')).toHaveTextContent('9.8 GB')
    await expect(canvas.getByTestId('processes-tile-network-received')).toHaveTextContent('1.2 MB/s')
  }}
/>

<!-- The first reading has no rates: dashes, and empty meters. -->
<Story
  name="The first reading"
  args={{ overview: FIRST_READING }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-tile-cpu-value')).toHaveTextContent('—')
    await expect(canvas.getByTestId('processes-tile-network-received')).toHaveTextContent('—')
  }}
/>

<Story name="No swap" args={{ overview: { ...OVERVIEW, swap: { total: 0, used: 0 } } }} />
