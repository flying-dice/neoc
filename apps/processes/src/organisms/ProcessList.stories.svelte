<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Overview, ProcessRef, ProcessRow } from 'app:rpc'
  import ProcessList from './ProcessList.svelte'
  import UsageTiles from './UsageTiles.svelte'
  import { OVERVIEW, ROWS } from './storyData'
  import { sorted, type Sort } from '../lib/processes.svelte'

  interface Args {
    overview: Overview
    rows: readonly ProcessRow[]
    total: number
    query: string
    sort: Sort
    selected: ProcessRef | null
    back: boolean
    onsearch: (text: string) => void
    onsort: (sort: Sort) => void
    onopen: (process: ProcessRow) => void
    onback: () => void
  }

  const onsearch = fn<(text: string) => void>()
  const onsort = fn<(sort: Sort) => void>()
  const onopen = fn<(process: ProcessRow) => void>()
  const onback = fn<() => void>()

  /** The processes, busiest first: what to look for, in what order, and a row for each. */
  const { Story } = defineMeta({
    title: 'Organisms/ProcessList',
    component: ProcessList,
    tags: ['autodocs'],
    args: { overview: OVERVIEW, rows: sorted(ROWS, 'cpu'), total: ROWS.length, query: '', sort: 'cpu', selected: null, back: false, onsearch, onsort, onopen, onback },
    beforeEach: () => {
      onsearch.mockClear()
      onsort.mockClear()
      onopen.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:380px;height:640px;background:var(--bg-grouped-elevated);color:var(--label);font:var(--type-body)">
    <ProcessList {...args}>
      {#snippet tiles()}<UsageTiles overview={args.overview} />{/snippet}
    </ProcessList>
  </div>
{/snippet}

<Story
  name="Busiest first"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('processes-row-2310')).toBeVisible()
    await userEvent.click(canvas.getByTestId('processes-row-812'))
    await expect(onopen).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="One is shown on the right" args={{ selected: { pid: 812, startTime: 1_700_000_812 } }} />

<Story name="With the way back" args={{ back: true }} />

<!-- Nothing matches what was typed: the list says so, and says what was looked for. -->
<Story
  name="No match"
  args={{ rows: [], query: 'postgress' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('processes-empty')).toHaveProperty('message', 'Nothing matches “postgress”. Check the name or PID.')
  }}
/>
