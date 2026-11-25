<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Detail, Related } from 'app:rpc'
  import ProcessInspector from './ProcessInspector.svelte'
  import { POSTGRES } from './storyData'
  import type { Ending, Refusal } from '../lib/processes.svelte'

  interface Args {
    detail: Detail
    canTerminate: boolean
    endable: boolean
    unavailable: string | null
    ending: boolean
    asking: Ending | null
    refused: Refusal | null
    notice: string | null
    back: boolean
    onback: () => void
    onask: (ending: Ending) => void
    onconfirm: () => void
    onkeep: () => void
    onfollow: (related: Related) => void
  }

  const onback = fn<() => void>()
  const onask = fn<(ending: Ending) => void>()
  const onconfirm = fn<() => void>()
  const onkeep = fn<() => void>()
  const onfollow = fn<(related: Related) => void>()

  /** One process in full, and the two ways to end it. */
  const { Story } = defineMeta({
    title: 'Organisms/ProcessInspector',
    component: ProcessInspector,
    tags: ['autodocs'],
    args: {
      detail: POSTGRES,
      canTerminate: true,
      endable: true,
      unavailable: null,
      ending: false,
      asking: null,
      refused: null,
      notice: null,
      back: false,
      onback,
      onask,
      onconfirm,
      onkeep,
      onfollow
    },
    beforeEach: () => {
      onback.mockClear()
      onask.mockClear()
      onconfirm.mockClear()
      onkeep.mockClear()
      onfollow.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:460px;height:700px;background:var(--bg-grouped-elevated);color:var(--label);font:var(--type-body)">
    <ProcessInspector {...args} />
  </div>
{/snippet}

<Story
  name="A database"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('processes-detail-state')).toHaveTextContent('PID 812 · Running')
    await userEvent.click(canvas.getByTestId('processes-terminate'))
    await expect(onask).toHaveBeenCalledWith('terminate')
    await userEvent.click(canvas.getByTestId('processes-force'))
    await expect(onask).toHaveBeenCalledWith('force')
    await userEvent.click(canvas.getByTestId('processes-child-841'))
    await expect(onfollow).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="With the way back" args={{ back: true }} />

<!-- The system has no request to quit (Windows): only the forced stop is there. -->
<Story name="No request to quit" args={{ canTerminate: false }} />

<Story name="Asked before force" tags={['keep-focus']} args={{ asking: 'force' }} />

<Story name="Not ended" args={{ refused: { name: 'postgres', message: 'This system did not let Processes stop postgres. It belongs to a user with more rights than yours: ask an administrator.' } }} />

<!-- Looked at, not ended: the system would not let Processes hold it with the right to end it. -->
<Story
  name="Cannot be ended"
  args={{
    detail: { ...POSTGRES, name: 'wininit', control: { available: false, reason: 'This system did not let Processes stop wininit. It belongs to a user with more rights than yours: ask an administrator.' } },
    endable: false,
    unavailable: 'This system did not let Processes stop wininit. It belongs to a user with more rights than yours: ask an administrator.'
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-unavailable')).toBeVisible()
    await expect(canvas.getByTestId('processes-detail-cpu')).toBeVisible()
    await expect(canvas.getByTestId('processes-force')).toHaveAttribute('disabled')
  }}
/>

<Story name="Asked to quit" args={{ notice: 'Asked postgres to quit.' }} />

<!-- It exited: what is shown is what it was, and nothing can be done to it. -->
<Story
  name="Has quit"
  args={{ detail: { pid: 812, startTime: 1_700_000_812, observation: 812, name: 'postgres', alive: false, control: { available: false, reason: 'postgres has quit.' }, command: [], children: [] }, endable: false }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-quit')).toBeVisible()
    await expect(canvas.getByTestId('processes-force')).toHaveAttribute('disabled')
  }}
/>

<Story name="No CPU yet" args={{ detail: { ...POSTGRES, cpu: undefined } }} />
