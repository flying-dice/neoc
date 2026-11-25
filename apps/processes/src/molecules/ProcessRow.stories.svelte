<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { ProcessRow } from 'app:rpc'
  import ProcessRowItem from './ProcessRow.svelte'

  interface Args {
    process: ProcessRow
    selected: boolean
    onpress: () => void
  }

  const onpress = fn<() => void>()

  /** One process: its name, PID and user, and its CPU and memory in columns that line up. */
  const { Story } = defineMeta({
    title: 'Molecules/ProcessRow',
    component: ProcessRowItem,
    tags: ['autodocs'],
    args: { process: { pid: 2310, startTime: 1, observation: 2310, name: 'luau-runtime', user: 'vos', cpu: 38.2, memory: 612 * 1024 ** 2 }, selected: false, onpress },
    beforeEach: () => onpress.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ui-list variant="insetGrouped" selection="single" label="Processes" style="width:420px">
    <ui-section header="Processes">
      <ProcessRowItem {...args} />
      <ProcessRowItem process={{ pid: 702, startTime: 2, observation: 702, name: 'sshd', user: 'root', cpu: 0.4, memory: 14 * 1024 ** 2 }} onpress={() => {}} />
    </ui-section>
  </ui-list>
{/snippet}

<Story
  name="A busy process"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('processes-row-2310'))
    await expect(onpress).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Shown on the right" args={{ selected: true }} />

<!-- The CPU of a process needs two readings: until then it is a dash. -->
<Story name="No CPU yet" args={{ process: { pid: 2310, startTime: 1, observation: 2310, name: 'luau-runtime', memory: 612 * 1024 ** 2 } }} />
