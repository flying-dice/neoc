<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Todo = VosApp.Todos.Todo
  import TodoRow from './TodoRow.svelte'
  import { todo } from '../lib/fakeApi'

  interface Args {
    todo: Todo
    today: string
    listName?: string
    expanded?: boolean
    ontoggle: () => void
    onflag: () => void
    onexpand: () => void
    onrename: (title: string) => void
    onnotes: (notes: string) => void
    ondue: (due: string) => void
    onremove: () => void
  }

  const TODAY = '2026-06-09'
  const ontoggle = fn<() => void>()
  const onflag = fn<() => void>()
  const onexpand = fn<() => void>()
  const onrename = fn<(title: string) => void>()
  const onnotes = fn<(notes: string) => void>()
  const ondue = fn<(due: string) => void>()
  const onremove = fn<() => void>()
  const heard = [ontoggle, onflag, onexpand, onrename, onnotes, ondue, onremove]

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!

  /** One todo. It keeps nothing: the todo comes in, and what the person did to it goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/TodoRow',
    component: TodoRow,
    tags: ['autodocs'],
    args: { todo: todo(1, 'Buy milk'), today: TODAY, ontoggle, onflag, onexpand, onrename, onnotes, ondue, onremove },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:460px;margin:0;padding:0 16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <TodoRow {...args} />
  </ul>
{/snippet}

<!-- A todo with nothing set: its check, its title, and the two buttons. -->
<Story
  name="Plain"
  play={async ({ canvas }) => {
    await expect(input(canvas, 'todos-item-1-title')).toHaveValue('Buy milk')
    await expect(canvas.getByTestId('todos-item-1-check')).not.toHaveAttribute('checked')
    await expect(canvas.getByTestId('todos-item-1-flag')).not.toHaveAttribute('pressed')
    await expect(canvas.queryByTestId('todos-item-1-notes')).toBeNull()
  }}
/>

<Story
  name="Due today, flagged, with notes"
  args={{ todo: todo(1, 'Buy milk', { due: TODAY, flagged: true, notes: 'The blue carton, two litres' }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-item-1-due-label')).toHaveTextContent('Today')
    await expect(canvas.getByTestId('todos-item-1-flag')).toHaveAttribute('pressed')
    await expect(canvas.getByTestId('todos-item-1-notes-label')).toHaveTextContent('The blue carton')
  }}
/>

<!-- Its day has passed: the day is red. -->
<Story name="Overdue" args={{ todo: todo(1, 'Renew the passport', { due: '2026-06-01' }) }} />

<!-- Among todos of several lists, it says which list it is in. -->
<Story
  name="In a smart list"
  args={{ todo: todo(1, 'Write the report', { due: '2026-06-10' }), listName: 'Work' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-item-1-list')).toHaveTextContent('Work')
    await expect(canvas.getByTestId('todos-item-1-due-label')).toHaveTextContent('Tomorrow')
  }}
/>

<Story
  name="Completed"
  args={{ todo: todo(1, 'Buy milk', { done: true, due: '2026-06-01' }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-item-1-check')).toHaveAttribute('checked')
  }}
/>

<Story name="A long title and long notes" args={{ todo: todo(1, 'Ask the landlord about the boiler, the window in the hall and the deposit', { notes: 'He said to call after six, and to have the contract and the meter readings at hand' }) }} />

<!-- The details are open: the notes, the day, and the way to delete it. -->
<Story
  name="Details open"
  args={{ todo: todo(1, 'Buy milk', { due: TODAY, notes: 'The blue carton' }), expanded: true }}
  play={async ({ canvas }) => {
    await expect(input(canvas, 'todos-item-1-notes')).toHaveValue('The blue carton')
    await expect(canvas.getByTestId('todos-item-1-due')).toHaveValue(TODAY)
    await expect(canvas.getByTestId('todos-item-1-details')).toHaveAttribute('pressed')
  }}
/>

<Story
  name="What a person does to it"
  tags={['play-only']}
  args={{ todo: todo(1, 'Buy milk', { due: TODAY }), expanded: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('todos-item-1-check').shadowRoot!.querySelector('button')!)
    await expect(ontoggle).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('todos-item-1-flag'))
    await expect(onflag).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('todos-item-1-details'))
    await expect(onexpand).toHaveBeenCalledTimes(1)
    // Enter commits a field, as leaving it does
    await userEvent.type(input(canvas, 'todos-item-1-title'), ' today{Enter}')
    await expect(onrename).toHaveBeenCalledWith('Buy milk today')
    await userEvent.type(input(canvas, 'todos-item-1-notes'), 'Two litres{Enter}')
    await expect(onnotes).toHaveBeenCalledWith('Two litres')
    await userEvent.click(canvas.getByTestId('todos-item-1-no-due'))
    await expect(ondue).toHaveBeenCalledWith('')
    await userEvent.click(canvas.getByTestId('todos-item-1-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A title cannot be blank: the field takes back the one the todo has, and nothing goes out. -->
<Story
  name="A blank title is taken back"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    // the whole title selected, then deleted (the testing library cannot `clear` an input in a shadow tree)
    await userEvent.type(input(canvas, 'todos-item-1-title'), '{Backspace}{Enter}', { initialSelectionStart: 0, initialSelectionEnd: 1000 })
    await expect(input(canvas, 'todos-item-1-title')).toHaveValue('Buy milk')
    await expect(onrename).not.toHaveBeenCalled()
  }}
/>
