<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import UsageTile from './UsageTile.svelte'

  interface Args {
    heading: string
    value: string
    note?: string
    share?: number
    testid: string
  }

  /** One summary tile: a value in large type over a meter of how full it is. */
  const { Story } = defineMeta({
    title: 'Molecules/UsageTile',
    component: UsageTile,
    tags: ['autodocs'],
    args: { heading: 'Memory', value: '9.8 GB', note: 'of 16 GB', share: 61, testid: 'tile' },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:240px;padding:16px;background:var(--bg-grouped-elevated);color:var(--label);font:var(--type-body)">
    <UsageTile {...args} />
  </div>
{/snippet}

<Story
  name="Memory"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('tile-value')).toHaveTextContent('9.8 GB')
    await expect(canvas.getByTestId('tile-note')).toHaveTextContent('of 16 GB')
  }}
/>

<!-- Before there are two readings the value is a dash and the meter is empty. -->
<Story name="Not measured yet" args={{ heading: 'CPU', value: '—', note: '8 cores', share: undefined }} />
