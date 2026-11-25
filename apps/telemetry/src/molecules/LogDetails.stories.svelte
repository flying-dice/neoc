<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import LogDetails from './LogDetails.svelte'
  import { LOGS } from '../lib/fakeApi'
  import type { Log } from '../lib/telemetry'

  interface Args {
    log: Log
    header: string
  }

  /** One log in full: how severe, when, where, by which app, and its fields. */
  const { Story } = defineMeta({
    title: 'Molecules/LogDetails',
    component: LogDetails,
    tags: ['autodocs'],
    args: { log: LOGS[0]!, header: 'Log' },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" label="Log" style="width:420px"><LogDetails {...args} /></ui-list>
{/snippet}

<Story name="A backend's log with fields" />

<!-- An error the page put in the trace of the call before it: the footer says so. -->
<Story name="A render error on the page" args={{ log: LOGS[1]! }} />

<Story name="No fields" args={{ log: LOGS[3]! }} />
