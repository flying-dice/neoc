<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import SpanDetails from './SpanDetails.svelte'
  import { SPANS } from '../lib/fakeApi'
  import type { Span } from '../lib/telemetry'

  interface Args {
    span: Span
  }

  /** One span in full: where it ran, how it ended, when, its ids and what it carried. */
  const { Story } = defineMeta({
    title: 'Molecules/SpanDetails',
    component: SpanDetails,
    tags: ['autodocs'],
    args: { span: SPANS[1]! },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" label="Span" style="width:420px"><SpanDetails {...args} /></ui-list>
{/snippet}

<Story name="The host's span" />

<!-- A root has no parent. -->
<Story name="The page's span" args={{ span: SPANS[0]! }} />

<Story name="Failed" args={{ span: SPANS[5]! }} />
