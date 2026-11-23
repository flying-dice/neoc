<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog, pickerSelect, textArea } from '@vos/storybook/kit'
  import type { Folder, Note } from 'app:rpc'
  import NoteEditor from './NoteEditor.svelte'
  import { listed, note } from '../lib/fakeApi'

  interface Args {
    note?: Note
    text: string
    loaded?: boolean
    folders: readonly Folder[]
    back?: boolean
    onedit: (text: string) => void
    oncommit: () => void
    onpin: (note: Note) => void
    onmove: (note: Note, folder: number) => void
    onremove: (note: Note) => void
    onrestore: (note: Note) => void
    onpurge: (note: Note) => void
    onback?: () => void
  }

  const BODY = 'Shopping\nMilk and bread\nEggs'
  const EDITED = new Date(2026, 5, 9, 8, 5)
  const SHOPPING = listed(note(1, BODY, EDITED))
  const FOLDERS: Folder[] = [
    { id: 1, name: 'Notes' },
    { id: 2, name: 'Work' }
  ]

  const onedit = fn<(text: string) => void>()
  const onpin = fn<(note: Note) => void>()
  const onmove = fn<(note: Note, folder: number) => void>()
  const onremove = fn<(note: Note) => void>()
  const onrestore = fn<(note: Note) => void>()
  const onpurge = fn<(note: Note) => void>()

  /** One note. It keeps nothing but the question it asks before a note goes for good: the text comes in, what is typed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/NoteEditor',
    component: NoteEditor,
    tags: ['autodocs'],
    args: {
      note: SHOPPING,
      text: BODY,
      folders: FOLDERS,
      onedit,
      oncommit: fn(),
      onpin,
      onmove,
      onremove,
      onrestore,
      onpurge,
      onback: fn()
    },
    beforeEach: () => [onedit, onpin, onmove, onremove, onrestore, onpurge].forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:480px;height:420px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <NoteEditor {...args} />
  </div>
{/snippet}

<Story
  name="A note"
  play={async ({ canvas }) => {
    await expect(textArea(canvas.getByTestId('notes-text'))).toHaveValue(BODY)
    await expect(canvas.getByTestId('notes-edited')).toHaveTextContent('Edited 9 June 2026 at 08:05')
    await expect(pickerSelect(canvas.getByTestId('notes-move'))).toHaveValue('1')
  }}
/>

<Story
  name="A pinned note"
  args={{ note: { ...SHOPPING, pinned: true } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-pin')).toHaveAttribute('pressed')
  }}
/>

<!-- With one folder there is nowhere to move a note to. -->
<Story
  name="One folder"
  args={{ folders: FOLDERS.slice(0, 1) }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('notes-move')).toBeNull()
  }}
/>

<Story name="A new note" args={{ note: listed(note(2, '', EDITED)), text: '' }} />

<!-- Its text has not been read yet: there is nothing to type in. -->
<Story
  name="Being read"
  args={{ text: '', loaded: false }}
  play={async ({ canvas }) => {
    await expect(textArea(canvas.getByTestId('notes-text'))).toBeDisabled()
  }}
/>

<!-- A deleted note is read, not written: it can be recovered, or deleted for good. -->
<Story
  name="A deleted note"
  args={{ note: { ...SHOPPING, deleted: true } }}
  play={async ({ canvas }) => {
    await expect(textArea(canvas.getByTestId('notes-text'))).toHaveAttribute('readonly')
    await expect(canvas.getByTestId('notes-restore')).toBeVisible()
    await expect(canvas.queryByTestId('notes-pin')).toBeNull()
  }}
/>

<Story
  name="No note"
  args={{ note: undefined, text: '' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-none')).toBeVisible()
  }}
/>

<Story name="In a narrow window" args={{ back: true }} />

<Story
  name="Typing, pinning, moving and deleting"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(textArea(canvas.getByTestId('notes-text')), '!')
    await expect(onedit).toHaveBeenCalledWith(`${BODY}!`)
    await userEvent.click(canvas.getByTestId('notes-pin'))
    await expect(onpin).toHaveBeenCalledWith(SHOPPING)
    await userEvent.selectOptions(pickerSelect(canvas.getByTestId('notes-move')), '2')
    await expect(onmove).toHaveBeenCalledWith(SHOPPING, 2)
    await userEvent.click(canvas.getByTestId('notes-remove'))
    await expect(onremove).toHaveBeenCalledWith(SHOPPING)
  }}
/>

<!-- Deleting for good asks first. -->
<Story
  name="Deleting for good"
  tags={['play-only']}
  args={{ note: { ...SHOPPING, deleted: true } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notes-purge'))
    await expect(alertDialog(canvas.getByTestId('notes-confirm-purge'))).toHaveTextContent('It cannot be brought back.')
    await expect(onpurge).not.toHaveBeenCalled()
    await userEvent.click(alertButton(canvas.getByTestId('notes-confirm-purge'), 'action'))
    await expect(onpurge).toHaveBeenCalledTimes(1)
  }}
/>
