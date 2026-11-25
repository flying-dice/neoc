<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import type { Summary } from '@notebooks/rpc'
  import Switcher from './Switcher.svelte'

  interface Args {
    notes: readonly Summary[]
    testid: string
    onopen: (id: number) => void
    oncreate: (title: string) => void
    onclose: () => void
  }

  const onopen = fn<(id: number) => void>()
  const oncreate = fn<(title: string) => void>()
  const onclose = fn<() => void>()

  const summary = (id: number, title: string, excerpt: string): Summary => ({
    id,
    notebook: 1,
    title,
    excerpt,
    tags: [],
    pinned: false,
    updated: '2026-06-09T08:00:00.000Z'
  })
  const NOTES = [
    summary(1, 'Garden plan', 'The beds for this year.'),
    summary(2, 'Tomatoes', 'Sow indoors in March.'),
    summary(3, 'Herbs', 'Basil, thyme and chives.'),
    summary(4, 'Reading list', '')
  ]

  /** The quick switcher. It keeps what is typed and which row is marked: the note chosen, or the name to make, goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/Switcher',
    component: Switcher,
    tags: ['autodocs'],
    args: { notes: NOTES, testid: 'switcher', onopen, oncreate, onclose },
    beforeEach: () => {
      for (const mock of [onopen, oncreate, onclose]) mock.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:420px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <Switcher {...args} />
  </div>
{/snippet}

<!-- Nothing typed: every note, the first marked, and the field ready to type in. -->
<Story
  name="Opened"
  tags={['keep-focus']}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('switcher-input')).toHaveFocus())
    await expect(canvas.getByTestId('switcher-note-1')).toHaveAttribute('aria-selected', 'true')
    await expect(canvas.queryByTestId('switcher-create')).toBeNull()
  }}
/>

<Story name="No notes yet" args={{ notes: [] }} />

<!-- A name is typed: the notes it could mean, then the row that makes a note of that name. -->
<Story
  name="Finding and choosing"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('switcher-input'), 'to')
    await expect(canvas.getByTestId('switcher-note-2')).toHaveAttribute('aria-selected', 'true')
    await expect(canvas.queryByTestId('switcher-note-3')).toBeNull()
    await expect(canvas.getByTestId('switcher-create')).toHaveTextContent('Make “to”')
    await userEvent.keyboard('{ArrowDown}')
    await expect(canvas.getByTestId('switcher-create')).toHaveAttribute('aria-selected', 'true')
    await userEvent.keyboard('{ArrowUp}{Enter}')
    await expect(onopen).toHaveBeenCalledWith(2)
    await userEvent.clear(canvas.getByTestId('switcher-input'))
    await userEvent.type(canvas.getByTestId('switcher-input'), 'Cold frame{Enter}')
    await expect(oncreate).toHaveBeenCalledWith('Cold frame')
    // a name a note already has is not offered again
    await userEvent.clear(canvas.getByTestId('switcher-input'))
    await userEvent.type(canvas.getByTestId('switcher-input'), 'herbs')
    await expect(canvas.queryByTestId('switcher-create')).toBeNull()
    await userEvent.keyboard('{Escape}')
    await expect(onclose).toHaveBeenCalledTimes(1)
  }}
/>
