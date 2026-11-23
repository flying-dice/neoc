<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import { alertButton } from '@vos/storybook/kit'
  import type { List, Todo } from 'app:rpc'
  import TodoList from './TodoList.svelte'
  import { REMINDERS, todo } from '../lib/fakeApi'

  interface Args {
    title: string
    todos: readonly Todo[]
    today: string
    list?: List
    removable?: boolean
    listName?: (todo: Todo) => string
    expanded?: number | null
    draft: string
    addable?: boolean
    back?: boolean
    ontoggle: (todo: Todo) => void
    onflag: (todo: Todo) => void
    onexpand: (todo: Todo) => void
    onrename: (todo: Todo, title: string) => void
    onnotes: (todo: Todo, notes: string) => void
    ondue: (todo: Todo, due: string) => void
    onremove: (todo: Todo) => void
    ondraft: (text: string) => void
    onadd: () => void
    onremovelist?: (list: List) => void
    onback?: () => void
  }

  const TODAY = '2026-06-09'
  const ontoggle = fn<(todo: Todo) => void>()
  const onflag = fn<(todo: Todo) => void>()
  const onexpand = fn<(todo: Todo) => void>()
  const onrename = fn<(todo: Todo, title: string) => void>()
  const onnotes = fn<(todo: Todo, notes: string) => void>()
  const ondue = fn<(todo: Todo, due: string) => void>()
  const onremove = fn<(todo: Todo) => void>()
  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()
  const onremovelist = fn<(list: List) => void>()
  const onback = fn<() => void>()
  const heard = [ontoggle, onflag, onexpand, onrename, onnotes, ondue, onremove, ondraft, onadd, onremovelist, onback]

  const TODOS = [
    todo(1, 'Buy milk', { due: TODAY }),
    todo(2, 'Renew the passport', { due: '2026-06-01', flagged: true }),
    todo(3, 'Call the dentist', { notes: 'Ask for a morning' }),
    todo(4, 'Water the plants')
  ]

  /** One list. It keeps only whether it is asking before a delete: the todos come in, what the person did goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/TodoList',
    component: TodoList,
    tags: ['autodocs'],
    args: {
      title: 'Reminders',
      todos: TODOS,
      today: TODAY,
      list: REMINDERS,
      draft: '',
      ontoggle,
      onflag,
      onexpand,
      onrename,
      onnotes,
      ondue,
      onremove,
      ondraft,
      onadd,
      onremovelist,
      onback
    },
    argTypes: { listName: { control: false } },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <TodoList {...args} />
  </div>
{/snippet}

<Story
  name="A list with todos"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-title')).toHaveTextContent('Reminders')
    await expect(canvas.getByTestId('todos-count')).toHaveTextContent('4')
    await expect(canvas.getByTestId('todos-item-2-due-label')).toHaveTextContent('1 Jun 2026')
    // the last list cannot be deleted: no way to is shown
    await expect(canvas.queryByTestId('todos-delete-list')).toBeNull()
  }}
/>

<!-- Nothing in it yet: what to do. -->
<Story
  name="An empty list"
  args={{ todos: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-empty')).toHaveProperty('message', 'Type one below.')
  }}
/>

<!-- A smart list gathers todos from several lists: each says which it is in, and the list has no colour of its own. -->
<Story
  name="A smart list"
  args={{ title: 'All', list: undefined, listName: (t) => (t.id % 2 ? 'Reminders' : 'Work') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-item-2-list')).toHaveTextContent('Work')
  }}
/>

<!-- The completed todos: they can be brought back, and nothing is added here. -->
<Story
  name="Completed"
  args={{ title: 'Completed', list: undefined, addable: false, todos: TODOS.slice(0, 2).map((t) => ({ ...t, done: true })) }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('todos-new-input')).toBeNull()
  }}
/>

<Story
  name="Nothing completed"
  args={{ title: 'Completed', list: undefined, addable: false, todos: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-empty')).toHaveProperty('message', 'Nothing has been completed yet.')
  }}
/>

<Story name="A todo's details open" args={{ expanded: 3 }} />

<!-- A window too narrow for the lists too: the way back to them. -->
<Story
  name="With the way back"
  args={{ back: true, removable: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Deleting a list deletes its todos: it asks first. -->
<Story
  name="Asking before a list is deleted"
  args={{ removable: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-delete-list'))
    // an alert over the window, not a line in it
    const alert = canvas.getByTestId('todos-confirm').shadowRoot!.querySelector('dialog')!
    await waitFor(() => expect(alert.matches(':modal')).toBe(true))
    await expect(alert).toHaveTextContent('Delete Reminders?')
    await expect(alert).toHaveTextContent('Its todos are deleted with it.')
    await expect(onremovelist).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Deleting a list, and thinking again"
  tags={['play-only']}
  args={{ removable: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-delete-list'))
    await userEvent.click(alertButton(canvas.getByTestId('todos-confirm'), 'cancel'))
    await expect(canvas.queryByTestId('todos-confirm')).toBeNull()
    await expect(onremovelist).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('todos-delete-list'))
    await userEvent.click(alertButton(canvas.getByTestId('todos-confirm'), 'action'))
    await expect(onremovelist).toHaveBeenCalledWith(REMINDERS)
    await expect(canvas.queryByTestId('todos-confirm')).toBeNull()
  }}
/>

<Story
  name="What goes out"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-item-1-flag'))
    await expect(onflag).toHaveBeenCalledWith(TODOS[0])
    await userEvent.click(canvas.getByTestId('todos-item-3-details'))
    await expect(onexpand).toHaveBeenCalledWith(TODOS[2])
    await userEvent.click(canvas.getByTestId('todos-item-4-check').shadowRoot!.querySelector('button')!)
    await expect(ontoggle).toHaveBeenCalledWith(TODOS[3])
  }}
/>
