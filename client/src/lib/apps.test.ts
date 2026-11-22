import { beforeEach, describe, expect, test } from 'bun:test'
import {
  AGENTS_APP_ID,
  appOf,
  apps,
  dockOrder,
  installedApp,
  onAppsRemoved,
  setInstalledApps
} from './apps.svelte'
import type { AppSummary } from '../shell/hostApi'

const summary = (id: string, extra: Partial<AppSummary> = {}): AppSummary => ({
  id,
  name: id[0].toUpperCase() + id.slice(1),
  version: '1.0.0',
  state: 'running',
  source: 'file',
  client: false,
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

  test('an app that is not installed is not there (the Agents app on a host without it)', () => {
    expect(appOf(AGENTS_APP_ID)).toBeUndefined()
    setInstalledApps([summary(AGENTS_APP_ID)])
    expect(appOf(AGENTS_APP_ID)?.id).toBe('agents')
    setInstalledApps([summary(AGENTS_APP_ID, { state: 'disabled' })])
    expect(appOf(AGENTS_APP_ID)).toBeUndefined()
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
      Object.keys(apps).filter(
        (id) => !['discover', 'settings', 'files', 'gallery', 'terminal'].includes(id)
      )
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

describe('dockOrder', () => {
  test('built-ins first, then the running installed apps, then Settings', () => {
    setInstalledApps([
      summary('calculator'),
      summary('notes', { state: 'disabled' }),
      summary('agents')
    ])
    expect(dockOrder()).toEqual([
      'discover',
      'files',
      'terminal',
      'calculator',
      'agents',
      'settings'
    ])
  })

  test('an empty host still has the built-ins', () => {
    expect(dockOrder()).toEqual(['discover', 'files', 'terminal', 'settings'])
  })
})
