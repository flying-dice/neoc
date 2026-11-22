import { describe, expect, test } from 'bun:test'
import { NOTEBOOK, Refused, SAMPLE, createFailingBackend, createFakeBackend, note, type FakeBackend } from './fakeApi'
import { Notebooks } from './notebooks.svelte'

const NOW = (): Date => new Date(2026, 5, 9, 9, 41)
const WORK = { id: 2, name: 'Work' } as const

/** A window on a fake backend, loaded and listening. It saves only when asked, so a test decides when. */
async function opened(backend: FakeBackend = createFakeBackend({ notes: SAMPLE })): Promise<Notebooks> {
  const notebooks = new Notebooks(backend.rpc, backend.events, { now: NOW, autosave: null })
  notebooks.listen()
  await notebooks.load()
  return notebooks
}

/** Wait until `done()` holds (an event is answered by a read of the library). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const titles = (notebooks: Notebooks): string[] => notebooks.notes.map((n) => n.title)

describe('a window that has just opened', () => {
  test('a new library: one notebook, no notes, none open', async () => {
    const notebooks = await opened(createFakeBackend())
    expect(notebooks.notebooks).toEqual([NOTEBOOK])
    expect(notebooks.notebook).toEqual(NOTEBOOK)
    expect(notebooks.notes).toEqual([])
    expect(notebooks.note).toBeNull()
    expect(notebooks.browsing).toBe(true)
    expect(notebooks.failure).toBeNull()
  })

  test('lists the notes of the first notebook, the bookmarked one first, with their tags', async () => {
    const notebooks = await opened()
    expect(titles(notebooks)).toEqual(['Garden plan', 'Tomatoes', 'Herbs', 'Reading list', '2026-06-09'])
    expect(notebooks.tags[0]).toEqual({ tag: 'garden', count: 3 })
    expect(notebooks.graph.nodes).toHaveLength(5)
    expect(notebooks.graph.links).toContainEqual({ from: 2, to: 3 })
  })
})

describe('a note', () => {
  test('is opened with its text and the notes that link to it', async () => {
    const notebooks = await opened()
    await notebooks.open(3)
    expect(notebooks.note?.title).toBe('Herbs')
    expect(notebooks.body).toContain('Basil, thyme')
    expect(notebooks.backlinks.map((b) => b.title)).toEqual(['Garden plan', 'Tomatoes'])
    expect(notebooks.backlinks[1]?.excerpt).toContain('share a bed with basil')
    expect(notebooks.browsing).toBe(false)
    expect(notebooks.status).toBe('saved')
  })

  test('a new one is called Untitled, then Untitled 2, and opens to be written', async () => {
    const notebooks = await opened(createFakeBackend())
    notebooks.setMode('read')
    await notebooks.create()
    expect(notebooks.note?.title).toBe('Untitled')
    expect(notebooks.mode).toBe('edit')
    await notebooks.create()
    expect(notebooks.note?.title).toBe('Untitled 2')
    expect(titles(notebooks)).toEqual(['Untitled 2', 'Untitled'])
  })

  test('what is written is sent when asked, and its links and tags are read from it', async () => {
    const notebooks = await opened()
    await notebooks.open(5)
    notebooks.edit('Planted the [[Tomatoes]]. #diary')
    expect(notebooks.status).toBe('edited')
    expect(await notebooks.flush()).toBe(true)
    expect(notebooks.status).toBe('saved')
    expect(notebooks.notes.find((n) => n.id === 5)?.tags).toEqual(['diary'])
    expect(notebooks.graph.links).toContainEqual({ from: 5, to: 2 })
    await notebooks.open(2)
    expect(notebooks.backlinks.map((b) => b.title)).toContain('2026-06-09')
  })

  test('what is written is saved a moment after the last keystroke', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = new Notebooks(backend.rpc, backend.events, { now: NOW, autosave: 5 })
    await notebooks.load()
    await notebooks.open(5)
    notebooks.edit('One')
    notebooks.edit('One, two')
    await until(() => notebooks.status === 'saved')
    expect((await backend.rpc.read({ note: 5 })).body).toBe('One, two')
  })

  test('opening another note sends what was written in this one first', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(5)
    notebooks.edit('Kept')
    await notebooks.open(2)
    expect(notebooks.note?.id).toBe(2)
    expect((await backend.rpc.read({ note: 5 })).body).toBe('Kept')
  })

  test('a note is not left while what was written could not be saved', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = new Notebooks({ ...backend.rpc, save: () => Promise.reject(new Error('Connection lost')) }, backend.events, {
      now: NOW,
      autosave: null
    })
    await notebooks.load()
    await notebooks.open(5)
    notebooks.edit('Not lost')
    await notebooks.open(2)
    expect(notebooks.note?.id).toBe(5)
    expect(notebooks.body).toBe('Not lost')
    expect(notebooks.failure).toBe('Connection lost')
    expect(notebooks.status).toBe('edited')
  })

  test('a task is ticked on its line, and saved', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(1)
    await notebooks.tick(5)
    expect(notebooks.body).toContain('- [x] Clear the north bed')
    expect((await backend.rpc.read({ note: 1 })).body).toContain('- [x] Clear the north bed')
  })

  test('is bookmarked, and goes first in the list', async () => {
    const notebooks = await opened()
    await notebooks.open(4)
    await notebooks.pin()
    expect(notebooks.note?.pinned).toBe(true)
    expect(titles(notebooks).slice(0, 2)).toEqual(['Garden plan', 'Reading list'])
    await notebooks.pin()
    expect(notebooks.note?.pinned).toBe(false)
  })

  test('is deleted: the window goes on to the list, and the links to it lead nowhere', async () => {
    const notebooks = await opened()
    await notebooks.open(3)
    await notebooks.remove()
    expect(notebooks.note).toBeNull()
    expect(notebooks.browsing).toBe(true)
    expect(titles(notebooks)).not.toContain('Herbs')
    expect(notebooks.resolves('Herbs')).toBe(false)
  })
})

describe('links', () => {
  test('following a link opens the note it names, whatever its capitals', async () => {
    const notebooks = await opened()
    await notebooks.open(1)
    await notebooks.follow('tomatoes')
    expect(notebooks.note?.id).toBe(2)
  })

  test('following a link to no note makes the note, linked from where it was named', async () => {
    const notebooks = await opened()
    expect(notebooks.resolves('Book notes')).toBe(false)
    await notebooks.open(4)
    await notebooks.follow('Book notes')
    expect(notebooks.note?.title).toBe('Book notes')
    expect(notebooks.mode).toBe('edit')
    expect(notebooks.backlinks.map((b) => b.title)).toEqual(['Reading list'])
    expect(notebooks.resolves('book notes')).toBe(true)
  })

  test("today's note is named after the day, and is made once", async () => {
    const notebooks = await opened(createFakeBackend())
    await notebooks.daily()
    expect(notebooks.note?.title).toBe('2026-06-09')
    const id = notebooks.note?.id
    await notebooks.daily()
    expect(notebooks.note?.id).toBe(id)
    expect(notebooks.notes).toHaveLength(1)
  })

  test('renaming a note rewrites the links to it', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(3)
    await notebooks.rename(' Herb bed ')
    expect(notebooks.note?.title).toBe('Herb bed')
    expect(notebooks.titleProblem).toBeNull()
    expect((await backend.rpc.read({ note: 1 })).body).toContain('[[Herb bed|the herb bed]]')
    expect((await backend.rpc.read({ note: 2 })).body).toContain('[[Herb bed]]')
    expect(notebooks.backlinks.map((b) => b.title)).toEqual(['Garden plan', 'Tomatoes'])
  })
})

describe('a title that a note cannot have', () => {
  test('is not sent: what is wrong is said at the field, not as a failure of the window', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    let sent = 0
    const notebooks = new Notebooks({ ...backend.rpc, rename: (params) => (sent++, backend.rpc.rename(params)) }, backend.events, {
      now: NOW,
      autosave: null
    })
    await notebooks.load()
    await notebooks.open(3)
    await notebooks.rename('  ')
    expect(notebooks.titleProblem).toBe('A note needs a title.')
    await notebooks.rename('Herbs [dried]')
    expect(notebooks.titleProblem).toBe('A title cannot have [ ] | # or ^ in it.')
    await notebooks.rename('tomatoes')
    expect(notebooks.titleProblem).toBe('There is already a note called tomatoes.')
    expect(sent).toBe(0)
    expect(notebooks.failure).toBeNull()
    expect(notebooks.note?.title).toBe('Herbs')
  })

  test('the problem follows the typing until the title is right, and is gone once it is', async () => {
    const notebooks = await opened()
    await notebooks.open(3)
    notebooks.retitle('Tomatoes')
    expect(notebooks.titleProblem).toBeNull() // nothing is said before the field is left
    await notebooks.rename('Tomatoes')
    notebooks.retitle('Tomatoes #1')
    expect(notebooks.titleProblem).toBe('A title cannot have [ ] | # or ^ in it.')
    notebooks.retitle('Dried herbs')
    expect(notebooks.titleProblem).toBeNull()
  })

  test("the backend's refusal lands at the field too", async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = new Notebooks(
      { ...backend.rpc, rename: () => Promise.reject(new Refused('There is already a note called Basil.')) },
      backend.events,
      { now: NOW, autosave: null }
    )
    await notebooks.load()
    await notebooks.open(3)
    await notebooks.rename('Basil')
    expect(notebooks.titleProblem).toBe('There is already a note called Basil.')
    expect(notebooks.failure).toBeNull()
  })

  test('the same title in other capitals is a rename, and the same title is nothing', async () => {
    const notebooks = await opened()
    await notebooks.open(3)
    await notebooks.rename('Herbs')
    expect(notebooks.titleProblem).toBeNull()
    await notebooks.rename('HERBS')
    expect(notebooks.note?.title).toBe('HERBS')
  })
})

describe('finding notes', () => {
  test('a tag narrows the list; the same tag again shows them all', async () => {
    const notebooks = await opened()
    notebooks.filter('kitchen')
    expect(titles(notebooks)).toEqual(['Herbs'])
    notebooks.filter('kitchen')
    expect(titles(notebooks)).toHaveLength(5)
  })

  test('a tag no note has any more is let go of', async () => {
    const notebooks = await opened()
    notebooks.filter('kitchen')
    await notebooks.open(3)
    notebooks.edit('Basil.')
    await notebooks.flush()
    expect(notebooks.tag).toBeNull()
  })

  test('a search finds words in titles and in text, with the line they are on', async () => {
    const notebooks = await opened()
    await notebooks.search('basil')
    expect(notebooks.hits?.map((h) => h.title)).toEqual(['Tomatoes', 'Herbs'])
    expect(notebooks.hits?.[1]?.excerpt).toContain('Basil, thyme and chives')
    await notebooks.search('')
    expect(notebooks.hits).toBeNull()
  })

  test('an answer to words that were typed over is not shown', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const slow: FakeBackend['rpc']['search'] = async (params) => {
      if (params.query === 'bas') await Bun.sleep(20)
      return backend.rpc.search(params)
    }
    const notebooks = new Notebooks({ ...backend.rpc, search: slow }, backend.events, { now: NOW, autosave: null })
    await notebooks.load()
    const first = notebooks.search('bas')
    await notebooks.search('overstory')
    await first
    expect(notebooks.hits?.map((h) => h.title)).toEqual(['Reading list'])
  })
})

describe('notebooks', () => {
  test('a new notebook is shown, empty', async () => {
    const notebooks = await opened()
    await notebooks.open(1)
    notebooks.nameNotebook(' Work ')
    expect(await notebooks.addNotebook()).toBe(true)
    expect(notebooks.notebookDraft).toBe('')
    expect(notebooks.notebook).toEqual(WORK)
    expect(notebooks.notes).toEqual([])
    expect(notebooks.note).toBeNull()
  })

  test('a name another notebook has is not sent, and the problem goes when the name is typed over', async () => {
    const notebooks = await opened()
    notebooks.nameNotebook('notebook')
    expect(await notebooks.addNotebook()).toBe(false)
    expect(notebooks.notebookProblem).toBe('There is already a notebook called notebook.')
    expect(notebooks.failure).toBeNull()
    notebooks.nameNotebook('Work')
    expect(notebooks.notebookProblem).toBeNull()
  })

  test('each notebook has its own notes, and a link names a note of its own notebook only', async () => {
    const notebooks = await opened(createFakeBackend({ notebooks: [NOTEBOOK, WORK], notes: [...SAMPLE, note(9, 'Roadmap', '', { notebook: 2 })] }))
    await notebooks.selectNotebook(2)
    expect(titles(notebooks)).toEqual(['Roadmap'])
    expect(notebooks.resolves('Herbs')).toBe(false)
    await notebooks.follow('Herbs')
    expect(notebooks.note?.notebook).toBe(2)
    expect(notebooks.note?.id).not.toBe(3)
  })

  test('deleting the notebook that is shown deletes its notes and goes on to the first', async () => {
    const notebooks = await opened(createFakeBackend({ notebooks: [NOTEBOOK, WORK], notes: [...SAMPLE, note(9, 'Roadmap', '', { notebook: 2 })] }))
    await notebooks.selectNotebook(2)
    await notebooks.open(9)
    await notebooks.removeNotebook()
    expect(notebooks.notebooks).toEqual([NOTEBOOK])
    expect(notebooks.notebook).toEqual(NOTEBOOK)
    expect(notebooks.note).toBeNull()
  })

  test('the last notebook cannot be deleted', async () => {
    const notebooks = await opened()
    await notebooks.removeNotebook()
    expect(notebooks.failure).toBe('The last notebook cannot be deleted.')
    expect(notebooks.notebooks).toEqual([NOTEBOOK])
  })
})

describe('changes from outside the window', () => {
  test('an agent writes a note: the list is read again', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await backend.rpc.create({ notebook: 1, title: 'From the agent' })
    backend.emit('notebooks.changed')
    await until(() => titles(notebooks).includes('From the agent'))
  })

  test('an agent rewrites the open note: it is read again', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(5)
    await backend.rpc.save({ note: 5, body: 'Rewritten.' })
    backend.emit('notebooks.changed')
    await until(() => notebooks.body === 'Rewritten.')
    expect(notebooks.status).toBe('saved')
  })

  test('what the person is writing is not replaced under them', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(5)
    notebooks.edit('Mine')
    await backend.rpc.save({ note: 5, body: 'Theirs' })
    backend.emit('host:reconnected')
    await Bun.sleep(10)
    expect(notebooks.body).toBe('Mine')
  })

  test('the open note was deleted elsewhere: the window goes on to the list', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    const notebooks = await opened(backend)
    await notebooks.open(5)
    await backend.rpc.remove({ note: 5 })
    backend.emit('notebooks.changed')
    await until(() => notebooks.note === null)
    expect(notebooks.browsing).toBe(true)
  })

  test('after `listen` is stopped, events are not heard', async () => {
    const backend = createFakeBackend()
    const notebooks = new Notebooks(backend.rpc, backend.events, { now: NOW, autosave: null })
    const stop = notebooks.listen()
    await notebooks.load()
    stop()
    await backend.rpc.create({ notebook: 1, title: 'Unheard' })
    backend.emit('notebooks.changed')
    await Bun.sleep(10)
    expect(notebooks.notes).toEqual([])
  })
})

describe('a backend that cannot be reached', () => {
  test('loading says why and shows nothing', async () => {
    const notebooks = await opened(createFailingBackend('The host is not answering'))
    expect(notebooks.failure).toBe('The host is not answering')
    expect(notebooks.notebooks).toEqual([])
    expect(notebooks.notebook).toBeUndefined()
  })

  test('the next call that works clears the failure', async () => {
    const backend = createFakeBackend({ notes: SAMPLE })
    let down = true
    const notebooks = new Notebooks(
      { ...backend.rpc, library: () => (down ? Promise.reject('offline') : backend.rpc.library()) },
      backend.events,
      { now: NOW, autosave: null }
    )
    await notebooks.load()
    expect(notebooks.failure).toBe('offline')
    down = false
    await notebooks.load()
    expect(notebooks.failure).toBeNull()
  })
})
