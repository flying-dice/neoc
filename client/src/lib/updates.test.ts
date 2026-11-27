import { describe, expect, test } from 'bun:test'
import { ACCESS_NOTE, updateApproval, updateFor, versionChange } from './updates'
import type { AppPreview, AppSummary } from '../shell/hostApi'

const installed: AppSummary = {
  id: 'notes',
  name: 'Notes',
  version: '0.4.0',
  state: 'running',
  source: 'this computer',
  permissions: ['clipboard-write']
}

const offer = (extra: Partial<AppPreview> = {}): AppPreview => ({
  url: 'file:///registry/notes/0.5.0/app-ref.json',
  sha256: 'a'.repeat(64),
  id: 'notes',
  name: 'Notes',
  version: '0.5.0',
  source: 'this computer',
  installed: { version: '0.4.0', state: 'running' },
  update: true,
  permissions: [],
  origins: [],
  ...extra
})

describe('updates', () => {
  test('the catalog offers an update only where the host marked one', () => {
    const newer = offer()
    const catalog = { apps: [offer({ id: 'todo', update: false }), newer], errors: [] }
    expect(updateFor(catalog, 'notes')).toBe(newer)
    expect(updateFor(catalog, 'todo')).toBeUndefined()
    expect(updateFor(catalog, 'calculator')).toBeUndefined()
  })

  test('the version change reads from the installed version to the offered one', () => {
    expect(versionChange('0.4.0', '0.5.0')).toBe('0.4.0 → 0.5.0')
  })

  test('the approval says what changes and what the new version asks for, new features marked', () => {
    const approval = updateApproval(
      installed,
      offer({ permissions: ['camera', 'clipboard-write'], origins: ['https://cdn.example.com'] })
    )
    expect(approval.title).toBe('Update Notes?')
    expect(approval.message).toBe(
      '0.4.0 → 0.5.0. Its window may use: camera (new), clipboard-write. ' +
        `Its page may load from: https://cdn.example.com. ${ACCESS_NOTE}`
    )
  })

  test('a version that asks for nothing more says only what changes and what any app can do', () => {
    expect(updateApproval(installed, offer()).message).toBe(`0.4.0 → 0.5.0. ${ACCESS_NOTE}`)
  })
})
