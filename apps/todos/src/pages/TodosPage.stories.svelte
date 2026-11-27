<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import TodosPage from './TodosPage.svelte'
  import { Todos } from '../lib/todos.svelte'
  import { REMINDERS, createFailingBackend, createFakeBackend, todo, type FakeBackend } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  // every story's today, whatever the day it is looked at
  const TODAY = '2026-06-09'
  const NOW = (): Date => new Date(2026, 5, 9, 9, 41)

  const busy = (): FakeBackend =>
    createFakeBackend({
      lists: [REMINDERS, { id: 2, name: 'Work', color: 'red' }, { id: 3, name: 'Groceries', color: 'green' }],
      todos: [
        todo(1, 'Renew the passport', { due: '2026-06-01', flagged: true }),
        todo(2, 'Call the dentist', { due: TODAY, notes: 'Ask for a morning' }),
        todo(3, 'Write the report', { list: 2, due: TODAY }),
        todo(4, 'Book the meeting room', { list: 2, due: '2026-06-12' }),
        todo(5, 'Milk', { list: 3 }),
        todo(6, 'Bread', { list: 3 }),
        todo(7, 'Send the invoice', { list: 2, done: true })
      ]
    })

  /** The Todos app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Todos',
    component: TodosPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = {
    click(element: Element): Promise<void>
    type(element: Element, text: string, options?: { initialSelectionStart: number; initialSelectionEnd: number }): Promise<void>
  }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const titled = (canvas: Canvas, title: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId('todos-title')).toHaveTextContent(title))
  const counts = (canvas: Canvas, id: string, count: number): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveProperty('value', String(count)))

  /** A narrow window shows the lists first: pick one there. In a wide one the same tap only changes the list shown. */
  async function show(canvas: Canvas, user: User, id: string): Promise<void> {
    await appears(canvas, id)
    await user.click(canvas.getByTestId(id))
  }
  async function add(canvas: Canvas, user: User, title: string): Promise<void> {
    // the testing library keeps its own idea of what the field holds, and misses the app emptying it after an add: type
    // over whatever it thinks is there
    await user.type(input(canvas, 'todos-new-input'), `${title}{Enter}`, { initialSelectionStart: 0, initialSelectionEnd: 1000 })
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const todos = new Todos(backend.rpc, backend.events, NOW)}
  <AppWindow
    title={app.title}
    width={args.width}
    height={args.height}
    minWidth={app.window.minW}
    minHeight={app.window.minH}
  >
    <TodosPage {todos} />
  </AppWindow>
{/snippet}

<!-- A new board: one list, nothing to do, and Today in front. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await appears(canvas, 'todos-list-1')
    await counts(canvas, 'todos-smart-all', 0)
  }}
/>

<!-- Lists and todos from before: Today shows what is due today and what is overdue, from every list. -->
<Story
  name="Today"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-smart-today')
    await titled(canvas, 'Today')
    await appears(canvas, 'todos-item-1')
    await expect(canvas.getByTestId('todos-count')).toHaveTextContent('3')
    await expect(canvas.getByTestId('todos-item-3-list')).toHaveTextContent('Work')
    await counts(canvas, 'todos-smart-scheduled', 4)
    await counts(canvas, 'todos-smart-completed', 1)
    await counts(canvas, 'todos-list-3', 2)
  }}
/>

<Story
  name="A list of the person's own"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-list-2')
    await titled(canvas, 'Work')
    await appears(canvas, 'todos-item-4')
    await expect(canvas.queryByTestId('todos-item-1')).toBeNull()
    // in its own list a todo does not say which list it is in
    await expect(canvas.queryByTestId('todos-item-4-list')).toBeNull()
    await expect(canvas.getByTestId('todos-delete-list')).toBeVisible()
  }}
/>

<Story
  name="Completed"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-smart-completed')
    await titled(canvas, 'Completed')
    await appears(canvas, 'todos-item-7')
    await expect(canvas.queryByTestId('todos-new-input')).toBeNull()
  }}
/>

<!-- A todo typed in a list is added to it, and the field is ready for the next. -->
<Story
  name="Adding a todo"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-list-1')
    await titled(canvas, 'Reminders')
    await add(canvas, userEvent, 'Buy milk')
    await appears(canvas, 'todos-item-1')
    await expect(input(canvas, 'todos-item-1-title')).toHaveValue('Buy milk')
    await expect(input(canvas, 'todos-new-input')).toHaveValue('')
    await counts(canvas, 'todos-list-1', 1)
  }}
/>

<!-- Ticked off, a todo leaves its list for Completed, and comes back from there. -->
<Story
  name="Completing a todo, and bringing it back"
  tags={['play-only']}
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-list-3')
    await appears(canvas, 'todos-item-5')
    await userEvent.click(canvas.getByTestId('todos-item-5-check').shadowRoot!.querySelector('button')!)
    await gone(canvas, 'todos-item-5')
    await expect(canvas.getByTestId('todos-count')).toHaveTextContent('1')
    if (canvas.queryByTestId('todos-back')) await userEvent.click(canvas.getByTestId('todos-back'))
    await show(canvas, userEvent, 'todos-smart-completed')
    await appears(canvas, 'todos-item-5')
    await userEvent.click(canvas.getByTestId('todos-item-5-check').shadowRoot!.querySelector('button')!)
    await gone(canvas, 'todos-item-5')
  }}
/>

<!-- A todo's details: its notes, its day, and the way to delete it. -->
<Story
  name="A todo's details"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-smart-today')
    await appears(canvas, 'todos-item-2-details')
    await userEvent.click(canvas.getByTestId('todos-item-2-details'))
    await appears(canvas, 'todos-item-2-notes')
    await expect(input(canvas, 'todos-item-2-notes')).toHaveValue('Ask for a morning')
    await expect(canvas.getByTestId('todos-item-2-due')).toHaveValue(TODAY)
  }}
/>

<!-- A list is named in the sidebar; it is shown as soon as it is there. -->
<Story
  name="Adding a list"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'todos-new-list-input')
    await userEvent.type(input(canvas, 'todos-new-list-input'), 'Groceries{Enter}')
    await titled(canvas, 'Groceries')
    await expect(canvas.getByTestId('todos-empty')).toBeVisible()
  }}
/>

<!-- Deleting a list asks first; then the list and its todos are gone. -->
<Story
  name="Deleting a list"
  tags={['play-only']}
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-list-2')
    await appears(canvas, 'todos-delete-list')
    await userEvent.click(canvas.getByTestId('todos-delete-list'))
    await userEvent.click(alertButton(canvas.getByTestId('todos-confirm'), 'action'))
    await gone(canvas, 'todos-list-2')
    await counts(canvas, 'todos-smart-all', 4)
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason, and nothing to add to. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'todos-smart-all')
    await waitFor(() => expect(canvas.getByTestId('todos-failure')).toHaveTextContent('The host is not answering'))
    await expect(canvas.queryByTestId('todos-new-input')).toBeNull()
  }}
/>
