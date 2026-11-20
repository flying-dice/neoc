import { describe, expect, test } from 'bun:test'
import { rowTestid, testid } from './testid'

describe('test ids derive from explicit keys (spec 5.4)', () => {
  test('the spec table', () => {
    expect(testid('calculator', 'main/_0/_1/_2/key-7')).toBe('calculator-key-7')
    expect(testid('notes', 'main/actions/save')).toBe('notes-actions-save')
    expect(testid('notes', 'main/list/@row/item/delete', 7)).toBe('notes-list-row-7-item-delete')
    expect(testid('notes', 'main/confirm-delete/@actions/confirm')).toBe(
      'notes-confirm-delete-confirm'
    )
  })
  test('auto keys and slot names never rename a node', () => {
    expect(testid('notes', 'main/_0/_1/@toolbar.trailing/new-note')).toBe('notes-new-note')
    expect(testid('notes', 'main/list')).toBe('notes-list')
    expect(testid('notes', 'main/_3/list')).toBe(testid('notes', 'main/list'))
  })
  test('the row container and an explicit override', () => {
    expect(rowTestid('notes', 'main/list', 7)).toBe('notes-list-row-7')
    expect(testid('notes', 'main/list/@row/item', 7, 'custom-id')).toBe('custom-id')
    expect(testid('notes', 'main/list', undefined, '')).toBe('notes-list')
  })
  test('row keys are never an index or text', () => {
    expect(testid('notes', 'main/list/@row/item', 'abc-123')).toBe('notes-list-row-abc-123-item')
  })
})
