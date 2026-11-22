<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import NotebooksPage from './NotebooksPage.svelte'
  import { Notebooks } from '../lib/notebooks.svelte'
  import { NOTEBOOK, SAMPLE, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  // every story's now, whatever the day it is looked at
  const NOW = (): Date => new Date(2026, 5, 9, 9, 41)
  const onlink = fn<(url: string) => void>()

  const used = (): FakeBackend => createFakeBackend({ notebooks: [NOTEBOOK, { id: 2, name: 'Work' }], notes: SAMPLE })

  /** The Notebooks app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Notebooks',
    component: NotebooksPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = {
    click(element: Element): Promise<void>
    type(element: Element, text: string, options?: { initialSelectionStart: number; initialSelectionEnd: number }): Promise<void>
    keyboard(text: string): Promise<void>
  }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const titled = (canvas: Canvas, title: string): Promise<void> => waitFor(() => expect(input(canvas, 'notebooks-title')).toHaveValue(title))
  const saved = (canvas: Canvas): Promise<void> => waitFor(() => expect(canvas.getByTestId('notebooks-status')).toHaveTextContent('Saved'))

  /** Open a note from the list (a narrow window shows the list first). */
  async function open(canvas: Canvas, user: User, id: number): Promise<void> {
    await appears(canvas, `notebooks-note-${id}`)
    await user.click(canvas.getByTestId(`notebooks-note-${id}`))
    await appears(canvas, 'notebooks-title')
  }
  /** Type over what a kit field holds. */
  const retype = (user: User, field: HTMLInputElement, text: string): Promise<void> =>
    user.type(field, text, { initialSelectionStart: 0, initialSelectionEnd: 1000 })
  /** A segment of the Write and Read control: the kit's control holds its segments in its shadow tree. */
  const segment = (canvas: Canvas, mode: 'edit' | 'read'): HTMLElement =>
    canvas.getByTestId('notebooks-mode').shadowRoot!.querySelector<HTMLElement>(`[data-testid="notebooks-mode-${mode}"]`)!
  async function read(canvas: Canvas, user: User): Promise<void> {
    await user.click(segment(canvas, 'read'))
    await appears(canvas, 'notebooks-reading')
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const notebooks = new Notebooks(backend.rpc, backend.events, { now: NOW, autosave: 50 })}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <NotebooksPage {notebooks} {onlink} />
  </AppWindow>
{/snippet}

<!-- A new library: one notebook with nothing in it, and no note open. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await appears(canvas, 'notebooks-empty')
    await expect(canvas.getByTestId('notebooks-notebook')).toHaveValue('1')
  }}
/>

<!-- Notes from before: the list, with the bookmarked note first, and the tags the notes have. -->
<Story
  name="A notebook with notes"
  args={{ backend: used }}
  play={async ({ canvas }) => {
    await appears(canvas, 'notebooks-note-1')
    await expect(canvas.getByTestId('notebooks-note-1')).toHaveProperty('icon', 'star')
    await expect(canvas.getByTestId('notebooks-tag-garden')).toBeVisible()
  }}
/>

<!-- A note as it reads: its links, its tasks, and the notes that link to it. -->
<Story
  name="Reading a note"
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 1)
    await titled(canvas, 'Garden plan')
    await read(canvas, userEvent)
    await expect(canvas.getByTestId('notebooks-reading-link-Tomatoes')).toBeVisible()
    await expect(canvas.getByTestId('notebooks-backlink-3')).toHaveTextContent('Herbs')
  }}
/>

<!-- A new note opens to be written; what is written is saved, and its tag is in the sidebar. -->
<Story
  name="Writing a new note"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-new')
    await userEvent.click(canvas.getByTestId('notebooks-new'))
    await titled(canvas, 'Untitled')
    await userEvent.type(canvas.getByTestId('notebooks-editor'), 'First thoughts. #ideas')
    await saved(canvas)
    if (canvas.queryByTestId('notebooks-back')) await userEvent.click(canvas.getByTestId('notebooks-back'))
    await appears(canvas, 'notebooks-tag-ideas')
    await expect(canvas.getByTestId('notebooks-note-1')).toHaveProperty('subheading', 'First thoughts. #ideas')
  }}
/>

<!-- A link is followed to its note; a link to no note makes the note, which knows where it was named. -->
<Story
  name="Following links"
  tags={['play-only']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 4)
    await read(canvas, userEvent)
    await userEvent.click(canvas.getByTestId('notebooks-reading-link-Book notes'))
    await titled(canvas, 'Book notes')
    await appears(canvas, 'notebooks-editor')
    await expect(canvas.getByTestId('notebooks-backlink-4')).toHaveTextContent('Reading list')
    await userEvent.click(canvas.getByTestId('notebooks-backlink-4'))
    await titled(canvas, 'Reading list')
  }}
/>

<!-- A task ticked in the reading view is ticked in the note's text. -->
<Story
  name="Ticking a task"
  tags={['play-only']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 1)
    await read(canvas, userEvent)
    await userEvent.click(canvas.getByTestId('notebooks-reading-task-5'))
    await waitFor(() => expect(canvas.getByTestId('notebooks-reading-task-5')).toBeChecked())
    await userEvent.click(segment(canvas, 'edit'))
    await waitFor(() => expect((canvas.getByTestId('notebooks-editor') as HTMLTextAreaElement).value).toContain('- [x] Clear the north bed'))
  }}
/>

<!-- A title another note has is said at the field, and not as a failure of the window. -->
<Story
  name="A title that is taken"
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 3)
    await retype(userEvent, input(canvas, 'notebooks-title'), 'Tomatoes{Enter}')
    await waitFor(() => expect(canvas.getByTestId('notebooks-title-problem')).toHaveTextContent('There is already a note called Tomatoes.'))
    await expect(canvas.getByTestId('notebooks-title')).toHaveAttribute('invalid')
    await expect(canvas.queryByTestId('notebooks-failure')).toBeNull()
  }}
/>

<!-- Words are looked for in titles and in text; what is found is listed with the line the words are on. -->
<Story
  name="Searching"
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.type(input(canvas, 'notebooks-search'), 'basil')
    await appears(canvas, 'notebooks-hit-3')
    await expect(canvas.getByTestId('notebooks-hit-2')).toHaveProperty('heading', 'Tomatoes')
    await expect(canvas.queryByTestId('notebooks-hit-1')).toBeNull()
  }}
/>

<!-- A tag narrows the list to the notes that have it. -->
<Story
  name="Narrowed to a tag"
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-tag-kitchen')
    await userEvent.click(canvas.getByTestId('notebooks-tag-kitchen'))
    await gone(canvas, 'notebooks-note-1')
    await expect(canvas.getByTestId('notebooks-note-3')).toBeVisible()
  }}
/>

<!-- The graph of the notebook; a dot opens its note. -->
<Story
  name="The graph"
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.click(canvas.getByTestId('notebooks-graph-toggle'))
    await appears(canvas, 'notebooks-graph')
    await expect(canvas.getByTestId('notebooks-graph-count')).toHaveTextContent('5 notes, 5 links')
  }}
/>

<Story
  name="Opening a note from the graph"
  tags={['play-only']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.click(canvas.getByTestId('notebooks-graph-toggle'))
    await appears(canvas, 'notebooks-node-2')
    await userEvent.click(canvas.getByTestId('notebooks-node-2'))
    await titled(canvas, 'Tomatoes')
  }}
/>

<!-- The quick switcher, over the window: a name is typed, and Return opens the note. It is as wide as the page lets it be,
     and a story's page is wider than its window: it is shown in the window the contract asks for, not the smallest. -->
<Story
  name="The quick switcher"
  tags={['keep-focus', '!app']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.click(canvas.getByTestId('notebooks-switch'))
    await appears(canvas, 'notebooks-switcher-input')
    await userEvent.type(canvas.getByTestId('notebooks-switcher-input'), 'her')
    await expect(canvas.getByTestId('notebooks-switcher-note-3')).toHaveAttribute('aria-selected', 'true')
  }}
/>

<Story
  name="Opening a note by name, and today's note"
  tags={['play-only']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.click(canvas.getByTestId('notebooks-switch'))
    await appears(canvas, 'notebooks-switcher-input')
    await userEvent.type(canvas.getByTestId('notebooks-switcher-input'), 'her{Enter}')
    await titled(canvas, 'Herbs')
    await gone(canvas, 'notebooks-switcher')
    if (canvas.queryByTestId('notebooks-back')) await userEvent.click(canvas.getByTestId('notebooks-back'))
    await userEvent.click(canvas.getByTestId('notebooks-daily'))
    await titled(canvas, '2026-06-09')
  }}
/>

<!-- A notebook is added and shown, empty; deleting it asks first, and goes back to the first. -->
<Story
  name="Adding and deleting a notebook"
  tags={['play-only']}
  args={{ backend: used }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notebooks-note-1')
    await userEvent.click(canvas.getByTestId('notebooks-add-notebook'))
    await userEvent.type(input(canvas, 'notebooks-new-notebook-input'), 'work{Enter}')
    await waitFor(() => expect(canvas.getByTestId('notebooks-new-notebook-problem')).toHaveTextContent('There is already a notebook called work.'))
    await retype(userEvent, input(canvas, 'notebooks-new-notebook-input'), 'Recipes{Enter}')
    await appears(canvas, 'notebooks-empty')
    await expect(canvas.getByTestId('notebooks-notebook')).toHaveValue('3')
    await userEvent.click(canvas.getByTestId('notebooks-delete-notebook'))
    await userEvent.click(canvas.getByTestId('notebooks-confirm-notebook-delete'))
    await appears(canvas, 'notebooks-note-1')
    await expect(canvas.getByTestId('notebooks-notebook')).toHaveValue('1')
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason, and nothing to start a note in. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('notebooks-failure')).toHaveTextContent('The host is not answering'))
    await expect(canvas.getByTestId('notebooks-new')).toHaveAttribute('disabled')
  }}
/>
