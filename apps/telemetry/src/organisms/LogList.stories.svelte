<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import LogList from './LogList.svelte'
  import { LOGS } from '../lib/fakeApi'
  import { logKey, type Level, type Log } from '../lib/telemetry'

  interface Args {
    logs: readonly Log[]
    query: string
    level: Level
    selected: string | null
    back: boolean
    onsearch: (text: string) => void
    onlevel: (level: Level) => void
    onopen: (log: Log) => void
    onback: () => void
  }

  const onsearch = fn<(text: string) => void>()
  const onlevel = fn<(level: Level) => void>()
  const onopen = fn<(log: Log) => void>()
  const onback = fn<() => void>()
  const NEWEST_FIRST = [...LOGS].reverse()

  /** Every app's logs, newest first, at the severity chosen. */
  const { Story } = defineMeta({
    title: 'Organisms/LogList',
    component: LogList,
    tags: ['autodocs'],
    args: { logs: NEWEST_FIRST, query: '', level: 'info', selected: logKey(NEWEST_FIRST[1]!), back: false, onsearch, onlevel, onopen, onback },
    beforeEach: () => {
      onsearch.mockClear()
      onlevel.mockClear()
      onopen.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:380px;height:560px;color:var(--label);font:var(--type-body)"><LogList {...args} /></div>
{/snippet}

<Story
  name="Every app's logs"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('telemetry-log-0'))
    await expect(onopen).toHaveBeenCalledWith(NEWEST_FIRST[0])
  }}
/>

<Story name="Errors only" args={{ level: 'error', logs: NEWEST_FIRST.filter((l) => l.severity === 'error') }} />

<Story name="Nothing logged" args={{ logs: [], selected: null }} />

<Story name="Nothing matches" args={{ logs: [], query: 'timeout', selected: null }} />
