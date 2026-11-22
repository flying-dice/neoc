<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { List } from 'app:rpc'
  import ListSidebar from './ListSidebar.svelte'
  import { REMINDERS } from '../lib/fakeApi'
  import { listView, smart, type SmartId, type View } from '../lib/views'

  interface Args {
    lists: readonly List[]
    counts: Readonly<Record<SmartId, number>>
    remaining: ReadonlyMap<number, number>
    selected?: View
    draft: string
    onselect: (view: View) => void
    ondraft: (name: string) => void
    onadd: () => void
  }

  const onselect = fn<(view: View) => void>()
  const ondraft = fn<(name: string) => void>()
  const onadd = fn<() => void>()

  const NONE = { today: 0, scheduled: 0, all: 0, flagged: 0, completed: 0 }
  const LISTS: List[] = [
    REMINDERS,
    { id: 2, name: 'Work', color: 'red' },
    { id: 3, name: 'A name long enough that it has to be cut short in the sidebar', color: 'green' }
  ]

  /** The lists. It keeps nothing: a list tapped, a name typed and the wish to add a list go out. */
  const { Story } = defineMeta({
    title: 'Organisms/ListSidebar',
    component: ListSidebar,
    tags: ['autodocs'],
    args: { lists: [REMINDERS], counts: NONE, remaining: new Map(), draft: '', onselect, ondraft, onadd },
    beforeEach: () => {
      onselect.mockClear()
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:280px;height:520px;color:var(--label);font:var(--type-body)">
    <ListSidebar {...args} />
  </div>
{/snippet}

<!-- A new board: the smart lists, all empty, and the one list. -->
<Story
  name="A new board"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-smart-today')).toHaveProperty('value', '0')
    await expect(canvas.getByTestId('todos-list-1')).toHaveProperty('heading', 'Reminders')
  }}
/>

<Story
  name="With lists and counts"
  args={{
    lists: LISTS,
    counts: { today: 2, scheduled: 5, all: 12, flagged: 1, completed: 34 },
    remaining: new Map([
      [1, 7],
      [2, 5]
    ]),
    selected: smart('today')
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-smart-today')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('todos-smart-all')).toHaveProperty('value', '12')
    await expect(canvas.getByTestId('todos-list-2')).toHaveProperty('value', '5')
    await expect(canvas.getByTestId('todos-list-3')).toHaveProperty('value', '0')
  }}
/>

<Story
  name="A list of the person's own selected"
  args={{ lists: LISTS, selected: listView(2) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-list-2')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('todos-smart-today')).not.toHaveAttribute('selected')
  }}
/>

<Story name="A list being named" args={{ draft: 'Groceries' }} />

<Story
  name="Tapping"
  tags={['play-only']}
  args={{ lists: LISTS, draft: 'Groceries' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-smart-flagged'))
    await expect(onselect).toHaveBeenCalledWith(smart('flagged'))
    await userEvent.click(canvas.getByTestId('todos-list-2'))
    await expect(onselect).toHaveBeenCalledWith(listView(2))
    await userEvent.click(canvas.getByTestId('todos-new-list-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
  }}
/>
