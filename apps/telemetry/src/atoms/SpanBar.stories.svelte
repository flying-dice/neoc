<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import SpanBar from './SpanBar.svelte'

  interface Args {
    left: number
    width: number
    tint: string
    failed: boolean
  }

  /** Where a span sits on its trace's time line. */
  const { Story } = defineMeta({
    title: 'Atoms/SpanBar',
    component: SpanBar,
    tags: ['autodocs'],
    args: { left: 0.1, width: 0.6, tint: 'var(--ios-purple)', failed: false },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:320px;padding:16px;background:var(--bg-grouped-elevated)"><SpanBar {...args} /></div>
{/snippet}

<Story name="Part of the trace" />

<Story name="The whole trace" args={{ left: 0, width: 1, tint: 'var(--ios-blue)' }} />

<!-- A span far shorter than its trace still shows. -->
<Story name="A sliver" args={{ left: 0.8, width: 0.01, tint: 'var(--ios-teal)' }} />

<Story name="Failed" args={{ failed: true }} />
