<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import LogRow from './LogRow.svelte'
  import { LOGS } from '../lib/fakeApi'
  import type { Log } from '../lib/telemetry'

  interface Args {
    log: Log
    showApp: boolean
    selected: boolean
    onpress: () => void
    testid: string
  }

  const onpress = fn<() => void>()

  /** One log: what it says, how severe it is, and when, where and by which app it was written. */
  const { Story } = defineMeta({
    title: 'Molecules/LogRow',
    component: LogRow,
    tags: ['autodocs'],
    args: { log: LOGS[0]!, showApp: false, selected: false, onpress, testid: 'telemetry-log-0' },
    beforeEach: () => onpress.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" selection="single" label="Logs" style="width:380px">
    <ui-section header="Logs"><LogRow {...args} /></ui-section>
  </ui-list>
{/snippet}

<Story
  name="Info"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-log-0'))
    await expect(onpress).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Warning" args={{ log: LOGS[3]! }} />

<!-- The longest message the row must survive: a framework's error. -->
<Story name="Error" args={{ log: LOGS[1]! }} />

<Story name="With its app" args={{ log: LOGS[2]!, showApp: true }} />

<Story name="Picked" args={{ selected: true }} />
