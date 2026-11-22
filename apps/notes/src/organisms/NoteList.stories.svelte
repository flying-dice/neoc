<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Folder, Note } from 'app:rpc'
  import NoteList from './NoteList.svelte'
  import { listed, note } from '../lib/fakeApi'
  import { sections, type Section } from '../lib/views'

  interface Args {
    title: string
    sections: readonly Section[]
    now: Date
    selected?: number
    query: string
    folder?: Folder
    creatable?: boolean
    back?: boolean
    onopen: (note: Note) => void
    onsearch: (query: string) => void
    oncreate: () => void
    onremovefolder?: (folder: Folder) => void
    onback?: () => void
  }

  const NOW = new Date(2026, 5, 9, 9, 41)
  const NOTES = [
    note(3, 'Wifi password\nhunter2', new Date(2026, 2, 14, 12, 0), { pinned: true }),
    note(1, 'Shopping\nMilk and bread', new Date(2026, 5, 9, 8, 5)),
    note(5, '', new Date(2026, 5, 9, 7, 30)),
    note(2, 'Meeting\nAsk about the budget', new Date(2026, 5, 8, 18, 0)),
    note(6, 'Holiday ideas\nSomewhere with a long name that does not fit on the line', new Date(2026, 5, 4, 10, 0)),
    note(7, 'Last year', new Date(2025, 10, 2, 10, 0))
  ].map(listed)

  const onopen = fn<(note: Note) => void>()
  const oncreate = fn<() => void>()
  const onremovefolder = fn<(folder: Folder) => void>()

  /** The notes of what is shown, under their headings. It keeps nothing but the question it asks before a folder goes. */
  const { Story } = defineMeta({
    title: 'Organisms/NoteList',
    component: NoteList,
    tags: ['autodocs'],
    args: {
      title: 'All Notes',
      sections: sections(NOTES, NOW),
      now: NOW,
      selected: 1,
      query: '',
      onopen,
      onsearch: fn(),
      oncreate,
      onremovefolder,
      onback: fn()
    },
    beforeEach: () => {
      onopen.mockClear()
      oncreate.mockClear()
      onremovefolder.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:300px;height:560px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <NoteList {...args} />
  </div>
{/snippet}

<Story
  name="Notes under their headings"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('6 Notes')
    await expect(canvas.getByTestId('notes-item-1')).toHaveProperty('subheading', '08:05  Milk and bread')
    await expect(canvas.getByTestId('notes-item-5')).toHaveProperty('heading', 'New Note')
    await expect(canvas.getByTestId('notes-item-5')).toHaveProperty('subheading', '07:30  No additional text')
    await expect(canvas.getByTestId('notes-item-2')).toHaveProperty('subheading', 'Yesterday  Ask about the budget')
    await expect(canvas.getByTestId('notes-section-Pinned')).toBeVisible()
    await expect(canvas.getByTestId('notes-section-2025')).toBeVisible()
  }}
/>

<!-- A folder of the person's own can be deleted from its list. -->
<Story name="A folder" args={{ title: 'Work', folder: { id: 2, name: 'Work' }, sections: sections(NOTES.slice(1, 4), NOW) }} />

<Story
  name="No notes"
  args={{ sections: [], selected: undefined }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-empty')).toBeVisible()
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('0 Notes')
  }}
/>

<!-- Recently Deleted: nothing is started here. -->
<Story
  name="Recently Deleted"
  args={{ title: 'Recently Deleted', creatable: false, sections: sections(NOTES.slice(3, 4), NOW), selected: undefined }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('notes-new')).toBeNull()
    await expect(canvas.getByTestId('notes-count')).toHaveTextContent('1 Note')
  }}
/>

<Story name="Searching" args={{ query: 'budget', sections: sections(NOTES.slice(3, 4), NOW), selected: undefined }} />

<Story name="Nothing found" args={{ query: 'zebra', sections: [], selected: undefined }} />

<Story name="In a narrow window" args={{ back: true, selected: undefined }} />

<Story
  name="Opening and starting a note"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notes-item-2'))
    await expect(onopen).toHaveBeenCalledWith(NOTES[3])
    await userEvent.click(canvas.getByTestId('notes-new'))
    await expect(oncreate).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Deleting a folder asks first. -->
<Story
  name="Deleting the folder"
  tags={['play-only']}
  args={{ title: 'Work', folder: { id: 2, name: 'Work' } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notes-delete-folder'))
    await expect(canvas.getByTestId('notes-confirm-folder')).toHaveTextContent('Its notes go to Recently Deleted.')
    await expect(onremovefolder).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('notes-confirm-folder-delete'))
    await expect(onremovefolder).toHaveBeenCalledWith({ id: 2, name: 'Work' })
  }}
/>
