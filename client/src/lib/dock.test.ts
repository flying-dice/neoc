import { describe, expect, test } from 'bun:test'
import {
  DEFAULT_PINS,
  DockPins,
  MAX_RECENTS,
  parseStored,
  recentApps,
  STORAGE_KEY,
  type DockStorage
} from './dock.svelte'

/** A storage in memory, with a way to play another page's write. */
function memory(initial: string | null = null): DockStorage & {
  value: string | null
  writes: string[]
  fromAnotherPage(value: string | null): void
} {
  let listener: (value: string | null) => void = () => {}
  const store = {
    value: initial,
    writes: [] as string[],
    read: () => store.value,
    write: (value: string) => {
      store.value = value
      store.writes.push(value)
    },
    onChange: (l: (value: string | null) => void) => {
      listener = l
    },
    fromAnotherPage: (value: string | null) => {
      store.value = value
      listener(value)
    }
  }
  return store
}

const stored = (pinned: unknown): string => JSON.stringify({ pinned })

describe('parseStored', () => {
  test('nothing stored: Discover, Files and Settings', () => {
    expect(parseStored(null)).toEqual(['discover', 'files', 'settings'])
    expect(DEFAULT_PINS).toEqual(['discover', 'files', 'settings'])
  })

  test('what was stored, in its order', () => {
    expect(parseStored(stored(['calculator', 'files']))).toEqual(['calculator', 'files'])
  })

  test('an empty list is a choice, not a damaged value', () => {
    expect(parseStored(stored([]))).toEqual([])
  })

  test.each(['', 'not json', 'null', '[]', '"files"', '{}', '{"pinned":"files"}', '{"pinned":{}}'])(
    '%p is not a list of pins: the defaults',
    (raw) => {
      expect(parseStored(raw)).toEqual([...DEFAULT_PINS])
    }
  )

  test('an entry that is not an id is skipped, and a repeated one counts once', () => {
    expect(
      parseStored(stored(['files', 7, null, '', 'notes', 'files', { id: 'x' }, 'x'.repeat(200)]))
    ).toEqual(['files', 'notes'])
  })

  test('a list too long to be a Dock is cut, not refused', () => {
    const many = Array.from({ length: 500 }, (_, i) => `app-${i}`)
    const parsed = parseStored(stored(many))
    expect(parsed).toHaveLength(64)
    expect(parsed[0]).toBe('app-0')
  })

  test('the key carries the version of the shape', () => {
    expect(STORAGE_KEY).toBe('vos.dock.v1')
  })
})

describe('DockPins', () => {
  test('starts from what is stored, or the defaults, and writes nothing by being read', () => {
    const empty = memory()
    expect(new DockPins(empty).pinned).toEqual(['discover', 'files', 'settings'])
    expect(empty.writes).toEqual([])
    const chosen = memory(stored(['notes']))
    expect(new DockPins(chosen).pinned).toEqual(['notes'])
    expect(chosen.writes).toEqual([])
  })

  test('with no storage the pins still work, for as long as the page', () => {
    const pins = new DockPins(null)
    pins.pin('notes')
    expect(pins.pinned).toEqual(['discover', 'files', 'settings', 'notes'])
  })

  test('a pin goes to the end, in the order they are added, and is stored', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    pins.pin('notes')
    pins.pin('calculator')
    expect(pins.pinned).toEqual(['discover', 'files', 'settings', 'notes', 'calculator'])
    expect(JSON.parse(storage.value!)).toEqual({
      pinned: ['discover', 'files', 'settings', 'notes', 'calculator']
    })
    expect(pins.isPinned('notes')).toBe(true)
  })

  test('pinning what is pinned changes nothing and writes nothing', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    pins.pin('files')
    expect(pins.pinned).toEqual(['discover', 'files', 'settings'])
    expect(storage.writes).toEqual([])
  })

  test('unpin removes one and keeps the order of the rest; the built-in pages can go too', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    pins.unpin('files')
    expect(pins.pinned).toEqual(['discover', 'settings'])
    expect(JSON.parse(storage.value!)).toEqual({ pinned: ['discover', 'settings'] })
    pins.unpin('nope')
    expect(storage.writes).toHaveLength(1)
  })

  test('every app unpinned stays that way on the next load', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    for (const id of [...pins.pinned]) pins.unpin(id)
    expect(new DockPins(storage).pinned).toEqual([])
  })

  test('an id that is not one is not pinned', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    pins.pin('')
    expect(pins.pinned).toEqual(['discover', 'files', 'settings'])
    expect(storage.writes).toEqual([])
  })

  test('an app that is not there is skipped where the Dock is drawn, and is back in its place when it returns', () => {
    const storage = memory(stored(['discover', 'calculator', 'files']))
    const pins = new DockPins(storage)
    const installed = new Set(['discover', 'files'])
    const exists = (id: string): boolean => installed.has(id)
    expect(pins.shown(exists)).toEqual(['discover', 'files'])
    expect(pins.pinned).toEqual(['discover', 'calculator', 'files']) // still the person's
    installed.add('calculator')
    expect(pins.shown(exists)).toEqual(['discover', 'calculator', 'files'])
  })

  test('a host that has not answered yet loses nobody a pin: a write made meanwhile keeps the absent ids', () => {
    const storage = memory(stored(['discover', 'calculator', 'notes']))
    const pins = new DockPins(storage)
    const nothingInstalledYet = (id: string): boolean => id === 'discover'
    expect(pins.shown(nothingInstalledYet)).toEqual(['discover'])
    expect(storage.writes).toEqual([]) // drawing the Dock never writes
    pins.pin('files')
    expect(JSON.parse(storage.value!)).toEqual({
      pinned: ['discover', 'calculator', 'notes', 'files']
    })
    pins.unpin('discover')
    expect(JSON.parse(storage.value!)).toEqual({ pinned: ['calculator', 'notes', 'files'] })
  })

  test('another page of this browser changes the pins: this one follows, and does not write back', () => {
    const storage = memory()
    const pins = new DockPins(storage)
    storage.fromAnotherPage(stored(['notes']))
    expect(pins.pinned).toEqual(['notes'])
    storage.fromAnotherPage(null) // the storage was cleared
    expect(pins.pinned).toEqual(['discover', 'files', 'settings'])
    expect(storage.writes).toEqual([])
  })
})

describe('recentApps', () => {
  const pinned = (id: string): boolean => ['discover', 'files'].includes(id)

  test('the open apps that are not pinned, most recently in front first', () => {
    const open = [
      { appId: 'discover', z: 11 },
      { appId: 'notes', z: 12 },
      { appId: 'calculator', z: 15 },
      { appId: 'files', z: 14 }
    ]
    expect(recentApps(open, pinned)).toEqual(['calculator', 'notes'])
  })

  test('no more than three, and the app in front is always one of them', () => {
    const open = ['a', 'b', 'c', 'd', 'e'].map((appId, i) => ({ appId, z: i }))
    expect(MAX_RECENTS).toBe(3)
    expect(recentApps(open, pinned)).toEqual(['e', 'd', 'c'])
    expect(recentApps(open, pinned, 1)).toEqual(['e'])
  })

  test('nothing open, or only pinned apps open: none', () => {
    expect(recentApps([], pinned)).toEqual([])
    expect(recentApps([{ appId: 'files', z: 3 }], pinned)).toEqual([])
  })

  test('an app counts once', () => {
    expect(
      recentApps(
        [
          { appId: 'notes', z: 1 },
          { appId: 'notes', z: 9 },
          { appId: 'todos', z: 5 }
        ],
        pinned
      )
    ).toEqual(['notes', 'todos'])
  })
})
