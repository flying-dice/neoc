import { beforeEach, describe, expect, test } from 'bun:test'
import { appOf, appOrder, apps, installedApp, onAppsRemoved, setInstalledApps } from './apps.svelte'
import type { AppSummary } from '../shell/hostApi'

const summary = (id: string, extra: Partial<AppSummary> = {}): AppSummary => ({
  id,
  name: id[0].toUpperCase() + id.slice(1),
  version: '1.0.0',
  state: 'running',
  source: 'file',
  permissions: [],
  ...extra
})

beforeEach(() => setInstalledApps([]))

describe('appOf', () => {
  test('finds a built-in page and a running installed app', () => {
    setInstalledApps([summary('notes', { tint: 'yellow', icon: 'doc' })])
    expect(appOf('files')?.name).toBe('Files')
    expect(appOf('notes')?.name).toBe('Notes')
  })

  test('an app that is not installed, or is disabled, is not there', () => {
    expect(appOf('notes')).toBeUndefined()
    setInstalledApps([summary('notes')])
    expect(appOf('notes')?.id).toBe('notes')
    setInstalledApps([summary('notes', { state: 'disabled' })])
    expect(appOf('notes')).toBeUndefined()
  })

  test.each(['constructor', '__proto__', 'toString', 'hasOwnProperty', '', 'nope'])(
    '%p is not an app, whatever the record inherits',
    (id) => {
      expect(appOf(id)).toBeUndefined()
    }
  )
})

describe('setInstalledApps', () => {
  test('only running apps get a definition, with the window and look they declared', () => {
    setInstalledApps([
      summary('calculator', { window: { w: 340, h: 560, minW: 320, minH: 500 }, tint: 'orange' }),
      summary('notes', { state: 'disabled' }),
      summary('broken', { state: 'failed', message: 'bad lua' })
    ])
    expect(
      Object.keys(apps).filter((id) => !['discover', 'settings', 'files'].includes(id))
    ).toEqual(['calculator'])
    expect(appOf('calculator')).toMatchObject({
      width: 340,
      height: 560,
      minWidth: 320,
      minHeight: 500
    })
    expect(installedApp('notes')?.state).toBe('disabled')
    expect(installedApp('broken')?.message).toBe('bad lua')
  })

  test('an app that declared no window gets the defaults, and an unknown icon a generic one', () => {
    setInstalledApps([summary('plain', { icon: 'no-such-symbol' })])
    expect(appOf('plain')).toMatchObject({
      width: 720,
      height: 520,
      minWidth: 320,
      minHeight: 240,
      icon: 'square.grid'
    })
  })

  test('the host cannot replace a built-in page by installing an app with its id', () => {
    setInstalledApps([summary('files', { name: 'Impostor' })])
    expect(appOf('files')?.name).toBe('Files')
  })

  test('listeners hear about apps that stopped running before they leave, so their windows can close', () => {
    setInstalledApps([summary('a'), summary('b')])
    const seen: { removed: string[]; stillThere: boolean }[] = []
    const off = onAppsRemoved((removed) =>
      seen.push({ removed, stillThere: removed.every((id) => !!appOf(id)) })
    )
    setInstalledApps([summary('a'), summary('b', { state: 'disabled' })])
    expect(seen).toEqual([{ removed: ['b'], stillThere: true }])
    expect(appOf('b')).toBeUndefined()
    off()
    setInstalledApps([])
    expect(seen).toHaveLength(1)
  })

  test('nothing removed, nobody told', () => {
    setInstalledApps([summary('a')])
    let told = 0
    onAppsRemoved(() => told++)
    setInstalledApps([summary('a'), summary('c')])
    expect(told).toBe(0)
  })
})

describe('appOrder', () => {
  test('the built-in pages in their fixed order, then the running installed apps by name', () => {
    setInstalledApps([
      summary('notes'),
      summary('calculator'),
      summary('todos', { state: 'disabled' }),
      summary('agents')
    ])
    expect(appOrder()).toEqual(['discover', 'files', 'settings', 'agents', 'calculator', 'notes'])
  })

  test('the order is by the name a person reads, not the id or the order the host listed them in', () => {
    setInstalledApps([
      summary('zz', { name: 'alpha' }),
      summary('aa', { name: 'Émile' }),
      summary('mm', { name: 'Beta' })
    ])
    expect(appOrder().slice(3)).toEqual(['zz', 'mm', 'aa'])
  })

  test('two apps with one name keep a fixed order (by id)', () => {
    setInstalledApps([summary('b', { name: 'Notes' }), summary('a', { name: 'Notes' })])
    expect(appOrder().slice(3)).toEqual(['a', 'b'])
  })

  test('an app with a built-in page’s id is not listed twice', () => {
    setInstalledApps([summary('files', { name: 'Impostor' })])
    expect(appOrder()).toEqual(['discover', 'files', 'settings'])
  })

  test('an empty host still has the built-ins', () => {
    expect(appOrder()).toEqual(['discover', 'files', 'settings'])
  })
})
