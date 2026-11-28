<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import LogInspector from './LogInspector.svelte'
  import { LOGS, RENDER_TRACE } from '../lib/fakeApi'
  import type { Log } from '../lib/telemetry'
  import type { Copied } from '../lib/telemetry.svelte'

  interface Args {
    log: Log
    copied: Copied
    back: boolean
    onback: () => void
    ontrace: (traceId: string) => void
    oncopy: () => void
  }

  const onback = fn<() => void>()
  const ontrace = fn<(traceId: string) => void>()
  const oncopy = fn<() => void>()

  /** One log in full, and the way to its trace. */
  const { Story } = defineMeta({
    title: 'Organisms/LogInspector',
    component: LogInspector,
    tags: ['autodocs'],
    args: { log: LOGS[1]!, copied: 'idle', back: false, onback, ontrace, oncopy },
    beforeEach: () => {
      onback.mockClear()
      ontrace.mockClear()
      oncopy.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:480px;height:560px;color:var(--label);font:var(--type-body)"><LogInspector {...args} /></div>
{/snippet}

<Story
  name="An error on the page"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-log-trace'))
    await expect(ontrace).toHaveBeenCalledWith(RENDER_TRACE)
  }}
/>

<Story
  name="Copying everything"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-log-copy'))
    await expect(oncopy).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Copied" args={{ copied: 'copied' }} />

<Story name="Copy refused" args={{ copied: 'refused' }} />

<Story name="A backend's log" args={{ log: LOGS[0]! }} />

<!-- A log written outside any call has no trace to go to. -->
<Story name="No trace" args={{ log: { ...LOGS[3]!, traceId: undefined, spanId: undefined } }} />

<Story name="Narrow" args={{ back: true }} />
