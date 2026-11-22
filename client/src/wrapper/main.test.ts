import { describe, expect, test } from 'bun:test'
import { appIdOf } from './main.svelte'

describe('the app a standalone page is for', () => {
  test('is the id in the path the host served it on', () => {
    expect(appIdOf('/apps/tasks')).toBe('tasks')
    expect(appIdOf('/apps/tasks/')).toBe('tasks')
    expect(appIdOf('/apps/my-app-2')).toBe('my-app-2')
  })
  test('is nothing for any other path, and never for a nested one', () => {
    for (const path of [
      '/',
      '/apps',
      '/apps/',
      '/apps/tasks/static/',
      '/apps/Tasks',
      '/apps/-x',
      '/frame/tasks',
      '/apps/a%2Fb'
    ]) {
      expect(appIdOf(path), path).toBeNull()
    }
  })
})
