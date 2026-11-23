<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, textArea } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import NotesPage from './NotesPage.svelte'
  import { Notes } from '../lib/notes.svelte'
  import { NOTES, createFailingBackend, createFakeBackend, note, type FakeBackend } from '../lib/fakeApi'

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

  const busy = (): FakeBackend =>
    createFakeBackend({
      now: NOW,
      folders: [NOTES, { id: 2, name: 'Work' }, { id: 3, name: 'Recipes' }],
      notes: [
        note(1, 'Shopping\nMilk and bread\nEggs', new Date(2026, 5, 9, 8, 5)),
        note(2, 'Meeting\nAsk about the budget', new Date(2026, 5, 8, 18, 0), { folder: 2 }),
        note(3, 'Wifi password\nhunter2', new Date(2026, 2, 14, 12, 0), { pinned: true }),
        note(4, 'Old plan\nNever mind', new Date(2026, 5, 7, 7, 0), { deleted: true }),
        note(5, 'Pancakes\n200g flour, 2 eggs, 300ml milk', new Date(2026, 5, 4, 10, 0), { folder: 3 }),
        note(6, 'Quarterly report\nDraft the summary first', new Date(2026, 4, 20, 10, 0), { folder: 2 })
      ]
    })

  /** The Notes app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Notes',
    component: NotesPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend({ now: NOW }) },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = { click(element: Element): Promise<void>; type(element: Element, text: string): Promise<void> }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const titled = (canvas: Canvas, title: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId('notes-title')).toHaveTextContent(title))
  const counts = (canvas: Canvas, id: string, count: number): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveProperty('value', String(count)))
  const says = (canvas: Canvas, text: string): Promise<void> => waitFor(() => expect(textArea(canvas.getByTestId('notes-text'))).toHaveValue(text))

  /** A narrow window shows one column at a time: step back until the folders are in front. In a wide one there is no way back to take. */
  async function toFolders(canvas: Canvas, user: User): Promise<void> {
    for (const id of ['notes-back-to-list', 'notes-back']) {
      const back = canvas.queryByTestId(id)
      if (back?.checkVisibility()) await user.click(back)
    }
  }
  async function show(canvas: Canvas, user: User, id: string): Promise<void> {
    await toFolders(canvas, user)
    await appears(canvas, id)
    await user.click(canvas.getByTestId(id))
  }
  async function open(canvas: Canvas, user: User, id: number): Promise<void> {
    await appears(canvas, `notes-item-${id}`)
    await user.click(canvas.getByTestId(`notes-item-${id}`))
    await appears(canvas, 'notes-text')
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const notes = new Notes(backend.rpc, backend.events, NOW)}
  <AppWindow
    title={definition.info.title}
    width={args.width}
    height={args.height}
    minWidth={app.window.minW}
    minHeight={app.window.minH}
  >
    <!-- a story saves as soon as something is typed: nothing waits on a timer -->
    <NotesPage {notes} saveAfter={0} />
  </AppWindow>
{/snippet}

<!-- A new board: one folder, no notes, and All Notes in front. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await titled(canvas, 'All Notes')
    await appears(canvas, 'notes-empty')
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('0 Notes')
  }}
/>

<!-- Folders and notes from before: the pinned note first, then by when each was edited. -->
<Story
  name="All Notes"
  args={{ backend: busy }}
  play={async ({ canvas }) => {
    await appears(canvas, 'notes-item-3')
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('5 Notes')
    await expect(canvas.getByTestId('notes-section-Pinned')).toBeVisible()
    await expect(canvas.getByTestId('notes-section-Today')).toBeVisible()
    await expect(canvas.getByTestId('notes-section-Previous 30 Days')).toBeVisible() // 20 May, from 9 June
    await expect(canvas.queryByTestId('notes-item-4')).toBeNull()
  }}
/>

<!-- A note is opened from the list: its text, when it was edited, and what can be done with it. -->
<Story
  name="A note open"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 1)
    await says(canvas, 'Shopping\nMilk and bread\nEggs')
    await expect(canvas.getByTestId('notes-edited')).toHaveTextContent('Edited 9 June 2026 at 08:05')
  }}
/>

<Story
  name="A folder of the person's own"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'notes-folder-2')
    await titled(canvas, 'Work')
    await appears(canvas, 'notes-item-6')
    await expect(canvas.queryByTestId('notes-item-1')).toBeNull()
    await expect(canvas.getByTestId('notes-delete-folder')).toBeVisible()
  }}
/>

<!-- A note started with the pencil is opened, and what is typed becomes its title in the list. -->
<Story
  name="Writing a note"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notes-new')
    await userEvent.click(canvas.getByTestId('notes-new'))
    await appears(canvas, 'notes-text')
    await userEvent.type(textArea(canvas.getByTestId('notes-text')), 'Ideas')
    await waitFor(() => expect(canvas.getByTestId('notes-item-1')).toHaveProperty('heading', 'Ideas'))
  }}
/>

<!-- Pinned, a note goes to the top of its list, under Pinned. -->
<Story
  name="Pinning a note"
  tags={['play-only']}
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 1)
    await userEvent.click(canvas.getByTestId('notes-pin'))
    await waitFor(() => expect(canvas.getByTestId('notes-pin')).toHaveAttribute('pressed'))
    await expect(canvas.getByTestId('notes-section-Pinned').contains(canvas.getByTestId('notes-item-1'))).toBe(true)
  }}
/>

<!-- Searching looks in every folder, whatever is shown. -->
<Story
  name="Searching"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'notes-search')
    await userEvent.type(input(canvas, 'notes-search'), 'budget')
    await titled(canvas, 'Search')
    await appears(canvas, 'notes-item-2')
    await gone(canvas, 'notes-item-1')
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('1 Note')
  }}
/>

<!-- A deleted note is kept in Recently Deleted: read, not written, until it is recovered. -->
<Story
  name="Recently Deleted"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'notes-deleted')
    await titled(canvas, 'Recently Deleted')
    await expect(canvas.queryByTestId('notes-new')).toBeNull()
    await open(canvas, userEvent, 4)
    await says(canvas, 'Old plan\nNever mind')
    await expect(textArea(canvas.getByTestId('notes-text'))).toHaveAttribute('readonly')
    await expect(canvas.getByTestId('notes-restore')).toBeVisible()
  }}
/>

<!-- Deleted, a note leaves its list for Recently Deleted, and comes back from there. -->
<Story
  name="Deleting a note, and recovering it"
  tags={['play-only']}
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await open(canvas, userEvent, 1)
    await userEvent.click(canvas.getByTestId('notes-remove'))
    await gone(canvas, 'notes-item-1')
    await show(canvas, userEvent, 'notes-deleted')
    await open(canvas, userEvent, 1)
    await userEvent.click(canvas.getByTestId('notes-restore'))
    await gone(canvas, 'notes-item-1')
    await show(canvas, userEvent, 'notes-all')
    await appears(canvas, 'notes-item-1')
  }}
/>

<!-- A folder is named in the sidebar; it is shown as soon as it is there. -->
<Story
  name="Adding a folder"
  play={async ({ canvas, userEvent }) => {
    await toFolders(canvas, userEvent)
    await appears(canvas, 'notes-new-folder-input')
    await userEvent.type(input(canvas, 'notes-new-folder-input'), 'Recipes{Enter}')
    await titled(canvas, 'Recipes')
    await expect(canvas.getByTestId('notes-empty')).toBeVisible()
  }}
/>

<!-- A name a folder has already is said under the field, not in a banner, and nothing is added. -->
<Story
  name="A folder name that is taken"
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await toFolders(canvas, userEvent)
    await appears(canvas, 'notes-new-folder-input')
    await userEvent.type(input(canvas, 'notes-new-folder-input'), 'work')
    await waitFor(() => expect(canvas.getByTestId('notes-new-folder-problem')).toHaveTextContent('There is already a folder called work.'))
    await expect(canvas.getByTestId('notes-new-folder-input')).toHaveAttribute('invalid')
    await expect(canvas.getByTestId('notes-new-folder-add')).toHaveAttribute('disabled')
    await expect(canvas.queryByTestId('notes-failure')).toBeNull()
  }}
/>

<!-- Deleting a folder asks first; then the folder is gone and its notes are in Recently Deleted. -->
<Story
  name="Deleting a folder"
  tags={['play-only']}
  args={{ backend: busy }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'notes-folder-2')
    await appears(canvas, 'notes-delete-folder')
    await userEvent.click(canvas.getByTestId('notes-delete-folder'))
    await userEvent.click(alertButton(canvas.getByTestId('notes-confirm-folder'), 'action'))
    await titled(canvas, 'All Notes')
    await toFolders(canvas, userEvent)
    await gone(canvas, 'notes-folder-2')
    await counts(canvas, 'notes-deleted', 3)
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason, and nothing to start a note in. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('notes-failure')).toHaveTextContent('The host is not answering'))
    await expect(canvas.queryByTestId('notes-new')).toBeNull()
  }}
/>
