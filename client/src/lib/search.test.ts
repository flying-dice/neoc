import { beforeEach, describe, expect, test } from 'bun:test'
import { setInstalledApps } from './apps.svelte'
import { recentlyOpened, spotlight } from './search.svelte'
import { settingsNavigation } from './settingsPanes.svelte'
import { windowManager as wm } from './windowManager.svelte'
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

beforeEach(() => {
  for (const win of [...wm.windows]) wm.close(win.id)
  spotlight.close()
  settingsNavigation.done()
  setInstalledApps([
    summary('calculator', { description: 'Arithmetic for you and your agents.' }),
    summary('notes', { state: 'disabled' })
  ])
})

const found = (query: string): string[] => {
  spotlight.show()
  spotlight.query = query
  return spotlight.results.map((r) => r.id)
}

describe('the shell’s search', () => {
  test('finds the built-in pages and the running installed apps, by name and by what they are for', () => {
    expect(found('calc')).toEqual(['app-calculator'])
    expect(found('arithmetic')).toEqual(['app-calculator'])
    expect(found('files')).toEqual(['app-files'])
    expect(found('notes')).toEqual([]) // installed, but disabled: it cannot be opened
  })

  test('opening an app’s result opens its window', () => {
    found('calc')
    expect(spotlight.activate()?.opens).toBe('calculator')
    expect(wm.focused?.appId).toBe('calculator')
    expect(spotlight.open).toBe(false)
  })

  test('opening a Settings pane opens Settings and asks it for that pane', () => {
    expect(found('appear')).toEqual(['settings-appearance'])
    spotlight.activate()
    expect(wm.focused?.appId).toBe('settings')
    expect(settingsNavigation.asked).toBe('appearance')
  })

  test('Show Home Screen leaves the stage, and is not offered on the Home Screen', () => {
    expect(found('home')).toEqual([])
    spotlight.close()
    wm.open('files')
    expect(found('home')).toEqual(['action-home'])
    expect(spotlight.activate()?.opens).toBeUndefined()
    expect(wm.atHome).toBe(true)
    expect(wm.isOpen('files')).toBe(true)
  })

  test('with nothing typed it suggests what was opened most recently', () => {
    recentlyOpened.note('settings')
    recentlyOpened.note('calculator')
    expect(found('').slice(0, 2)).toEqual(['app-calculator', 'app-settings'])
  })
})
