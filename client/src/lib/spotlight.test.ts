import { describe, expect, test } from 'bun:test'
import {
  builtinSources,
  MAX_RESULTS,
  RecentlyOpened,
  search,
  Spotlight,
  type SearchableApp,
  type SpotlightResult,
  type SpotlightShell,
  type SpotlightSource
} from './spotlight.svelte'

const app = (id: string, name: string, description?: string): SearchableApp => ({
  id,
  name,
  description,
  icon: 'square.grid'
})

const APPS: SearchableApp[] = [
  app('discover', 'Discover'),
  app('files', 'Files'),
  app('settings', 'Settings'),
  app('agents', 'Agents', 'Chat with the agents on this host.'),
  app('calculator', 'Calculator', 'Arithmetic for you and your agents.'),
  app('notes', 'Notes', 'Plain notes your agents can read and append to.'),
  app('quick', 'Quick Notes', 'Jot something down.'),
  app('keynotes', 'Keynotes', 'Slides.')
]

function shell(extra: Partial<SpotlightShell> = {}): SpotlightShell & { did: string[] } {
  const did: string[] = []
  return {
    did,
    apps: () => APPS,
    recent: () => [],
    openApp: (id) => did.push(`open ${id}`),
    openSettings: (pane) => did.push(`settings ${pane}`),
    atHome: () => false,
    showHome: () => did.push('home'),
    ...extra
  }
}

const ids = (results: SpotlightResult[]): string[] => results.map((r) => r.id)
const find = (query: string, s: SpotlightShell = shell()): string[] =>
  ids(search(builtinSources(s), query))

describe('the built-in sources', () => {
  test('an app is found by its name: its start first, then a word’s start, then anywhere', () => {
    expect(find('note')).toEqual(['app-notes', 'app-quick', 'app-keynotes'])
  })

  test('case and accents do not matter', () => {
    expect(find('CALC')).toEqual(['app-calculator'])
    expect(find('cálc')).toEqual(['app-calculator'])
  })

  test('an installed app is found by what it is for, below every name match', () => {
    // "Agents" by name, then the apps whose descriptions mention agents, in the Home Screen's order
    expect(find('agents')).toEqual(['app-agents', 'app-calculator', 'app-notes'])
    expect(search(builtinSources(shell()), 'arith')[0]).toMatchObject({
      id: 'app-calculator',
      rank: 3
    })
  })

  test('a built-in page is found by name', () => {
    expect(find('fil')).toEqual(['app-files'])
    expect(find('disc')).toEqual(['app-discover'])
  })

  test('a Settings pane is found by name, beside the app', () => {
    expect(find('appear')).toEqual(['settings-appearance'])
    expect(find('dev')).toEqual(['settings-devices'])
    expect(find('gene')).toEqual(['settings-general'])
    expect(find('set')).toEqual(['app-settings'])
  })

  test('a result says what it is and where it is drawn from', () => {
    const [pane] = search(builtinSources(shell()), 'appearance')
    expect(pane).toMatchObject({
      title: 'Appearance',
      kind: 'Settings',
      icon: 'sun',
      opens: 'settings'
    })
    const [calculator] = search(builtinSources(shell()), 'calc')
    expect(calculator).toMatchObject({ title: 'Calculator', kind: 'App', opens: 'calculator' })
  })

  test('running a result does what it is for', () => {
    const s = shell()
    const sources = builtinSources(s)
    search(sources, 'calc')[0].run()
    search(sources, 'appear')[0].run()
    search(sources, 'home')[0].run()
    expect(s.did).toEqual(['open calculator', 'settings appearance', 'home'])
  })

  test('Show Home Screen is offered while something covers the Home Screen, not when it is already on show', () => {
    expect(find('home')).toEqual(['action-home'])
    expect(find('show')).toEqual(['action-home'])
    expect(find('home', shell({ atHome: () => true }))).toEqual([])
    const [action] = search(builtinSources(shell()), 'home screen')
    expect(action).toMatchObject({ kind: 'Action', title: 'Show Home Screen' })
    expect(action.opens).toBeUndefined()
  })

  test('nothing matches: nothing', () => {
    expect(find('zzzz')).toEqual([])
  })

  test('nothing typed: the apps opened most recently, then the first of the Home Screen, four in all', () => {
    expect(find('')).toEqual(['app-discover', 'app-files', 'app-settings', 'app-agents'])
    expect(find('   ', shell({ recent: () => ['notes', 'files'] }))).toEqual([
      'app-notes',
      'app-files',
      'app-discover',
      'app-settings'
    ])
  })

  test('a recently opened app that is gone is not suggested', () => {
    expect(find('', shell({ recent: () => ['uninstalled', 'notes'] }))[0]).toBe('app-notes')
  })
})

describe('search', () => {
  const source = (id: string, results: Partial<SpotlightResult>[]): SpotlightSource => ({
    id,
    results: () =>
      results.map((r) => ({
        id: 'x',
        title: 'X',
        kind: 'Thing',
        icon: 'doc',
        rank: 0,
        run: () => {},
        ...r
      }))
  })

  test('another source’s results are ranked with the rest: by rank, then source order, then the source’s own order', () => {
    const notes = source('notes', [
      { id: 'note-1', rank: 1 },
      { id: 'note-2', rank: 0 }
    ])
    const todos = source('todos', [
      { id: 'todo-1', rank: 0 },
      { id: 'todo-2', rank: 1 }
    ])
    expect(ids(search([notes, todos], 'q'))).toEqual(['note-2', 'todo-1', 'note-1', 'todo-2'])
  })

  test('an id given twice is listed once, and the list has a limit', () => {
    const twice = source('a', [{ id: 'same' }, { id: 'same' }])
    expect(ids(search([twice, twice], 'q'))).toEqual(['same'])
    const many = source(
      'many',
      Array.from({ length: 30 }, (_, i) => ({ id: `r${i}` }))
    )
    expect(search([many], 'q')).toHaveLength(MAX_RESULTS)
    expect(search([many], 'q', 3)).toHaveLength(3)
  })

  test('a source is asked with the query trimmed', () => {
    const asked: string[] = []
    search([{ id: 's', results: (q) => (asked.push(q), []) }], '  calc  ')
    expect(asked).toEqual(['calc'])
  })
})

describe('RecentlyOpened', () => {
  test('most recent first, each app once', () => {
    const recent = new RecentlyOpened()
    for (const id of ['discover', 'files', 'notes', 'files']) recent.note(id)
    expect(recent.ids).toEqual(['files', 'notes', 'discover'])
  })
})

describe('Spotlight', () => {
  const opened = (): { spotlight: Spotlight; did: string[] } => {
    const s = shell({ recent: () => ['notes'] })
    const spotlight = new Spotlight(builtinSources(s))
    spotlight.show()
    return { spotlight, did: s.did }
  }

  test('closed: no results, nothing highlighted, nothing said', () => {
    const spotlight = new Spotlight(builtinSources(shell()))
    expect(spotlight.open).toBe(false)
    expect(spotlight.results).toEqual([])
    expect(spotlight.activeId).toBeNull()
    expect(spotlight.status).toBe('')
    expect(spotlight.activate()).toBeNull()
  })

  test('opens empty, with suggestions and the first highlighted', () => {
    const { spotlight } = opened()
    expect(spotlight.open).toBe(true)
    expect(spotlight.query).toBe('')
    expect(spotlight.suggesting).toBe(true)
    expect(ids(spotlight.results)).toEqual([
      'app-notes',
      'app-discover',
      'app-files',
      'app-settings'
    ])
    expect(spotlight.activeId).toBe('app-notes')
    expect(spotlight.status).toBe('4 suggestions')
  })

  test('typing filters as it goes and says how many were found', () => {
    const { spotlight } = opened()
    spotlight.query = 'not'
    expect(ids(spotlight.results)).toEqual(['app-notes', 'app-quick', 'app-keynotes'])
    expect(spotlight.suggesting).toBe(false)
    expect(spotlight.status).toBe('3 results')
    spotlight.query = 'calc'
    expect(spotlight.status).toBe('1 result')
    spotlight.query = 'zzzz'
    expect(spotlight.results).toEqual([])
    expect(spotlight.activeId).toBeNull()
    expect(spotlight.status).toBe('No results')
  })

  test('Down and Up move the highlight and stop at the ends', () => {
    const { spotlight } = opened()
    spotlight.query = 'note'
    expect(spotlight.activeId).toBe('app-notes')
    spotlight.move(-1)
    expect(spotlight.activeId).toBe('app-notes')
    spotlight.move(1)
    expect(spotlight.activeId).toBe('app-quick')
    spotlight.move(1)
    spotlight.move(1)
    expect(spotlight.activeId).toBe('app-keynotes')
    spotlight.move(-1)
    expect(spotlight.activeId).toBe('app-quick')
  })

  test('typing more puts the highlight back on the top result', () => {
    const { spotlight } = opened()
    spotlight.query = 'note'
    spotlight.move(1)
    spotlight.query = 'notes'
    expect(spotlight.activeId).toBe('app-notes')
  })

  test('the pointer highlights a result that is listed, and no other', () => {
    const { spotlight } = opened()
    spotlight.query = 'note'
    spotlight.highlight('app-keynotes')
    expect(spotlight.activeId).toBe('app-keynotes')
    spotlight.highlight('app-calculator')
    expect(spotlight.activeId).toBe('app-keynotes')
  })

  test('Enter opens the highlighted result (the top one by default), and the panel closes', () => {
    const { spotlight, did } = opened()
    spotlight.query = 'note'
    expect(spotlight.activate()?.opens).toBe('notes')
    expect(did).toEqual(['open notes'])
    expect(spotlight.open).toBe(false)
    expect(spotlight.query).toBe('')
  })

  test('a result that was moved to, or clicked, is the one opened', () => {
    const a = opened()
    a.spotlight.query = 'note'
    a.spotlight.move(1)
    a.spotlight.activate()
    expect(a.did).toEqual(['open quick'])
    const b = opened()
    b.spotlight.query = 'appear'
    expect(b.spotlight.activate('settings-appearance')?.opens).toBe('settings')
    expect(b.did).toEqual(['settings appearance'])
  })

  test('with no result, Enter opens nothing and the panel stays', () => {
    const { spotlight, did } = opened()
    spotlight.query = 'zzzz'
    expect(spotlight.activate()).toBeNull()
    expect(spotlight.open).toBe(true)
    expect(spotlight.query).toBe('zzzz')
    expect(did).toEqual([])
  })

  test('closing forgets what was typed: it opens empty the next time', () => {
    const { spotlight } = opened()
    spotlight.query = 'calc'
    spotlight.close()
    expect(spotlight.open).toBe(false)
    spotlight.show()
    expect(spotlight.query).toBe('')
    expect(spotlight.suggesting).toBe(true)
  })

  test('the shortcut opens it, and closes it when it is open', () => {
    const spotlight = new Spotlight(builtinSources(shell()))
    spotlight.toggle()
    expect(spotlight.open).toBe(true)
    spotlight.toggle()
    expect(spotlight.open).toBe(false)
  })

  test('showing a panel that is open keeps what was typed', () => {
    const { spotlight } = opened()
    spotlight.query = 'calc'
    spotlight.show()
    expect(spotlight.query).toBe('calc')
  })
})
