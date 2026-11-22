import { describe, expect, test } from 'bun:test'
import { NOTES, createFailingBackend, createFakeBackend, note, type FakeBackend } from './fakeApi'
import { Notes } from './notes.svelte'
import { ALL, DELETED, folderView } from './views'

const NOW = (): Date => new Date(2026, 5, 9, 9, 41)
const WORK = { id: 2, name: 'Work' } as const

const busy = (): FakeBackend =>
  createFakeBackend({
    now: NOW,
    folders: [NOTES, WORK],
    notes: [
      note(1, 'Shopping\nMilk and bread', new Date(2026, 5, 9, 8, 0)),
      note(2, 'Meeting\nAsk about the budget', new Date(2026, 5, 8, 18, 0), { folder: 2 }),
      note(3, 'Wifi password\nhunter2', new Date(2026, 2, 14, 12, 0), { pinned: true }),
      note(4, 'Old plan\nNever mind', new Date(2026, 5, 7, 7, 0), { deleted: true })
    ]
  })

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend({ now: NOW })): Promise<Notes> {
  const notes = new Notes(backend.rpc, backend.events, NOW)
  notes.listen()
  await notes.load()
  return notes
}

/** Wait until `done()` holds (an event is answered by a read of the board). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const ids = (notes: Notes): number[] => notes.notes.map((n) => n.id)
const listedAs = (notes: Notes, id: number): { title: string; preview: string } | undefined => {
  const found = notes.notes.find((n) => n.id === id)
  return found && { title: found.title, preview: found.preview }
}
const find = (notes: Notes, id: number) => {
  const found = notes.notes.find((n) => n.id === id)
  if (!found) throw new Error(`note ${id} is not in the list`)
  return found
}

describe('a window that has just opened', () => {
  test('shows All Notes, empty, with the one folder a new board has', async () => {
    const notes = await opened()
    expect(notes.folders).toEqual([NOTES])
    expect(notes.view).toEqual(ALL)
    expect(notes.title).toBe('All Notes')
    expect(notes.notes).toEqual([])
    expect(notes.note).toBeUndefined()
    expect(notes.failure).toBeNull()
    expect(notes.pane).toBe('list')
    expect(notes.creatable).toBe(true)
    expect(notes.removable).toBe(false)
  })

  test('counts what each folder holds, and lists the notes under their headings', async () => {
    const notes = await opened(busy())
    expect(notes.counts.all).toBe(3)
    expect(notes.counts.deleted).toBe(1)
    expect(notes.counts.folders.get(1)).toBe(2)
    expect(notes.counts.folders.get(2)).toBe(1)
    expect(notes.sections.map((s) => [s.name, s.notes.map((n) => n.id)])).toEqual([
      ['Pinned', [3]],
      ['Today', [1]],
      ['Yesterday', [2]]
    ])
  })
})

describe('writing a note', () => {
  test('a new note is started in the first folder from All Notes, and opened', async () => {
    const notes = await opened(busy())
    await notes.create()
    expect(notes.note).toMatchObject({ folder: 1, title: '', pinned: false, deleted: false })
    expect(notes.titleOf(notes.note!)).toBe('New Note')
    expect(notes.loaded).toBe(true)
    expect(notes.text).toBe('')
    expect(notes.pane).toBe('note')
  })

  test('in a folder it is started in that folder', async () => {
    const notes = await opened(busy())
    await notes.select(folderView(2))
    await notes.create()
    expect(notes.note?.folder).toBe(2)
  })

  test('what is typed is saved: its first line is the title, the next the preview', async () => {
    const notes = await opened()
    await notes.create()
    notes.text = 'Shopping\nMilk and bread'
    expect(notes.unsaved).toBe(true)
    await notes.save()
    expect(notes.unsaved).toBe(false)
    expect(listedAs(notes, notes.note!.id)).toEqual({ title: 'Shopping', preview: 'Milk and bread' })
  })

  test('text that is saved already is not sent again', async () => {
    const backend = busy()
    let writes = 0
    const write = backend.rpc.write
    const notes = await opened({ ...backend, rpc: { ...backend.rpc, write: (p) => (writes++, write(p)) } })
    await notes.open(find(notes, 1))
    await notes.save()
    notes.text = 'Shopping\nMilk, bread and eggs'
    await notes.save()
    await notes.save()
    expect(writes).toBe(1)
  })

  test('a note with nothing in it is not kept when it is left', async () => {
    const notes = await opened(busy())
    await notes.create()
    expect(notes.counts.all).toBe(4)
    await notes.select(ALL)
    expect(notes.counts.all).toBe(3)
    expect(notes.counts.deleted).toBe(1)
  })

  test('what is typed is saved when another note is opened', async () => {
    const notes = await opened(busy())
    await notes.open(find(notes, 1))
    notes.text = 'Shopping\nEggs'
    await notes.open(find(notes, 2))
    expect(notes.text).toBe('Meeting\nAsk about the budget')
    expect(listedAs(notes, 1)).toEqual({ title: 'Shopping', preview: 'Eggs' })
  })

  test('a save that fails keeps what is typed, to be saved again', async () => {
    const backend = busy()
    let down = true
    const write = backend.rpc.write
    const rpc = { ...backend.rpc, write: ((p) => (down ? Promise.reject(new Error('The host is not answering')) : write(p))) as typeof write }
    const notes = await opened({ ...backend, rpc })
    await notes.open(find(notes, 1))
    notes.text = 'Shopping\nEggs'
    await notes.save()
    expect(notes.failure).toBe('The host is not answering')
    expect(notes.unsaved).toBe(true)
    down = false
    await notes.save()
    expect(notes.failure).toBeNull()
    expect(listedAs(notes, 1)?.preview).toBe('Eggs')
  })
})

describe('opening a note', () => {
  test('reads its whole text', async () => {
    const notes = await opened(busy())
    expect(notes.loaded).toBe(false)
    await notes.open(find(notes, 3))
    expect(notes.note?.id).toBe(3)
    expect(notes.text).toBe('Wifi password\nhunter2')
    expect(notes.loaded).toBe(true)
    expect(notes.unsaved).toBe(false)
  })

  test('stepping back from it, in a narrow window, shows the list, then the folders', async () => {
    const notes = await opened(busy())
    await notes.open(find(notes, 1))
    expect(notes.pane).toBe('note')
    await notes.back()
    expect(notes.pane).toBe('list')
    expect(notes.note).toBeUndefined()
    await notes.back()
    expect(notes.pane).toBe('folders')
    await notes.select(folderView(2))
    expect(notes.pane).toBe('list')
    expect(notes.title).toBe('Work')
  })
})

describe('pinning, moving and deleting', () => {
  test('a pinned note is under Pinned; pinned again, it is not', async () => {
    const notes = await opened(busy())
    await notes.pin(find(notes, 1))
    expect(notes.sections[0]).toMatchObject({ name: 'Pinned' })
    expect(notes.sections[0]?.notes.map((n) => n.id)).toEqual([1, 3])
    await notes.pin(find(notes, 1))
    expect(notes.sections[0]?.notes.map((n) => n.id)).toEqual([3])
  })

  test('a note moves to another folder', async () => {
    const notes = await opened(busy())
    await notes.move(find(notes, 1), 2)
    expect(notes.counts.folders.get(2)).toBe(2)
    await notes.select(folderView(2))
    expect(ids(notes)).toEqual([1, 2])
  })

  test('a deleted note goes to Recently Deleted, unpinned, with what was typed in it', async () => {
    const notes = await opened(busy())
    await notes.open(find(notes, 3))
    notes.text = 'Wifi password\nhunter3'
    await notes.remove(notes.note!)
    expect(notes.note).toBeUndefined()
    expect(ids(notes)).toEqual([1, 2])
    await notes.select(DELETED)
    expect(notes.creatable).toBe(false)
    expect(listedAs(notes, 3)).toEqual({ title: 'Wifi password', preview: 'hunter3' })
    expect(find(notes, 3).pinned).toBe(false)
  })

  test('a deleted note is read, not written, and comes back when recovered', async () => {
    const notes = await opened(busy())
    await notes.select(DELETED)
    await notes.open(find(notes, 4))
    expect(notes.text).toBe('Old plan\nNever mind')
    notes.text = 'Typed over'
    await notes.save()
    expect(notes.failure).toBeNull()
    await notes.restore(notes.note!)
    expect(notes.notes).toEqual([])
    await notes.select(ALL)
    expect(listedAs(notes, 4)).toEqual({ title: 'Old plan', preview: 'Never mind' })
  })

  test('a deleted note deleted again is gone for good', async () => {
    const notes = await opened(busy())
    await notes.select(DELETED)
    await notes.purge(find(notes, 4))
    expect(notes.counts.deleted).toBe(0)
    expect(notes.counts.all).toBe(3)
  })
})

describe('searching', () => {
  test('finds the notes whose text holds what is typed, in every folder, whatever its capitals', async () => {
    const notes = await opened(busy())
    await notes.select(folderView(1))
    await notes.search('BUDGET')
    expect(notes.searching).toBe(true)
    expect(ids(notes)).toEqual([2])
  })

  test('does not find deleted notes, and shows the view again when the field is emptied', async () => {
    const notes = await opened(busy())
    await notes.search('never mind')
    expect(notes.notes).toEqual([])
    await notes.search('')
    expect(notes.searching).toBe(false)
    expect(ids(notes)).toEqual([1, 2, 3])
  })

  test('only the answer to what is typed now is kept', async () => {
    const notes = await opened(busy())
    const first = notes.search('milk')
    const second = notes.search('hunter')
    await Promise.all([first, second])
    expect(ids(notes)).toEqual([3])
  })
})

describe('folders', () => {
  test('a folder is added, shown, and the field emptied', async () => {
    const notes = await opened()
    notes.folderDraft = '  Recipes  '
    await notes.addFolder()
    expect(notes.folders.map((f) => f.name)).toEqual(['Notes', 'Recipes'])
    expect(notes.title).toBe('Recipes')
    expect(notes.folderDraft).toBe('')
    expect(notes.removable).toBe(true)
  })

  test('a name that is taken is said beside the field, and not sent', async () => {
    const backend = busy()
    let sent = 0
    const addFolder = backend.rpc.addFolder
    const notes = await opened({ ...backend, rpc: { ...backend.rpc, addFolder: (p) => (sent++, addFolder(p)) } })
    notes.folderDraft = 'work'
    expect(notes.folderProblem).toBe('There is already a folder called work.')
    await notes.addFolder()
    expect(sent).toBe(0)
    expect(notes.failure).toBeNull()
    notes.folderDraft = 'Workshop'
    expect(notes.folderProblem).toBeNull()
  })

  test("the backend's refusal lands beside the field, and goes when the name changes", async () => {
    const backend = busy()
    const refusal = Object.assign(new Error('There is already a folder called Home.'), { code: -32010 })
    const notes = await opened({ ...backend, rpc: { ...backend.rpc, addFolder: () => Promise.reject(refusal) } })
    notes.folderDraft = 'Home'
    await notes.addFolder()
    expect(notes.folderProblem).toBe('There is already a folder called Home.')
    expect(notes.folderDraft).toBe('Home')
    expect(notes.failure).toBeNull()
    notes.folderDraft = 'Home 2'
    expect(notes.folderProblem).toBeNull()
  })

  test('a failure that is not about the name is the window\'s', async () => {
    const backend = busy()
    const notes = await opened({ ...backend, rpc: { ...backend.rpc, addFolder: () => Promise.reject(new Error('The host is not answering')) } })
    notes.folderDraft = 'Home'
    await notes.addFolder()
    expect(notes.failure).toBe('The host is not answering')
    expect(notes.folderProblem).toBeNull()
    expect(notes.folderDraft).toBe('Home')
  })

  test('a deleted folder sends its notes to Recently Deleted, and the window on to All Notes', async () => {
    const notes = await opened(busy())
    await notes.select(folderView(2))
    await notes.open(find(notes, 2))
    await notes.removeFolder(WORK)
    expect(notes.folders).toEqual([NOTES])
    expect(notes.view).toEqual(ALL)
    expect(notes.note).toBeUndefined()
    expect(ids(notes)).toEqual([1, 3])
    expect(notes.counts.deleted).toBe(2)
  })
})

describe('changes made elsewhere', () => {
  test('an agent adds to the open note: the window reads it again', async () => {
    const backend = busy()
    const notes = await opened(backend)
    await notes.open(find(notes, 1))
    backend.rewrite(1, 'Shopping\nMilk and bread\nEggs')
    backend.emit('notes.changed')
    await until(() => notes.text.endsWith('Eggs'))
    expect(notes.unsaved).toBe(false)
  })

  test('what is being typed is not written over', async () => {
    const backend = busy()
    const notes = await opened(backend)
    await notes.open(find(notes, 1))
    notes.text = 'Shopping\nMine'
    backend.rewrite(1, 'Shopping\nTheirs')
    backend.emit('notes.changed')
    await until(() => listedAs(notes, 1)?.preview === 'Theirs')
    expect(notes.text).toBe('Shopping\nMine')
  })

  test('the board is read again when the host connection comes back', async () => {
    const backend = busy()
    const notes = await opened(backend)
    backend.rewrite(2, 'Meeting moved')
    backend.emit('host:reconnected')
    await until(() => listedAs(notes, 2)?.title === 'Meeting moved')
  })
})

describe('a backend that cannot be reached', () => {
  test('says why, and shows nothing', async () => {
    const notes = await opened(createFailingBackend('The host is not answering'))
    expect(notes.failure).toBe('The host is not answering')
    expect(notes.notes).toEqual([])
    expect(notes.creatable).toBe(false)
  })
})
