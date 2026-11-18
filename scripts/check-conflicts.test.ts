import { describe, expect, test } from 'bun:test'
import { findConflictMarkers, isBinary } from './check-conflicts'

// Fixtures are built from rows, never a template literal: a marker at the start of a line in this file would
// trip the lint on itself.
const lines = (...rows: string[]): string => rows.join('\n')
const OPEN = '<'.repeat(7)
const CLOSE = '>'.repeat(7)
const SEP = '='.repeat(7)

describe('findConflictMarkers', () => {
  test('a whole conflict block reports its three marker lines', () => {
    const text = lines('keep', `${OPEN} HEAD`, 'ours', SEP, 'theirs', `${CLOSE} branch`, 'keep')
    expect(findConflictMarkers(text).map((m) => m.line)).toEqual([2, 4, 6])
  })

  test('two blocks in one file', () => {
    const block = [`${OPEN} a`, SEP, `${CLOSE} b`]
    expect(findConflictMarkers(lines(...block, 'x', ...block)).map((m) => m.line)).toEqual([1, 2, 3, 5, 6, 7])
  })

  test('an unclosed block still fails on its opening marker', () => {
    expect(findConflictMarkers(lines(`${OPEN} HEAD`, 'ours'))).toEqual([{ line: 1, text: `${OPEN} HEAD` }])
  })

  test('CRLF files are read the same way', () => {
    expect(findConflictMarkers([`${OPEN} HEAD`, SEP, `${CLOSE} b`].join('\r\n')).map((m) => m.line)).toEqual([1, 2, 3])
  })

  test('a setext heading underline is not a conflict', () => {
    expect(findConflictMarkers(lines('Title', SEP, '', 'Other', '=====', '', 'More', '============'))).toEqual([])
  })

  test('a separator outside a block, or of another length, is not a conflict', () => {
    expect(findConflictMarkers(lines(SEP, 'x', `${SEP}=`, ` ${SEP}`, `${SEP} trailing`))).toEqual([])
  })

  test('only an exact seven-character separator counts inside a block', () => {
    const text = lines(`${OPEN} HEAD`, '========', '=====', SEP, `${CLOSE} b`)
    expect(findConflictMarkers(text).map((m) => m.line)).toEqual([1, 4, 5])
  })

  test('markers need their trailing space and the start of the line', () => {
    expect(findConflictMarkers(lines(OPEN, `  ${OPEN} x`, `a ${OPEN} b`, `${CLOSE} stray`, `${CLOSE}`))).toEqual([])
  })

  test('a stray closing marker without an opening one is ignored', () => {
    expect(findConflictMarkers(lines(`${CLOSE} quoted`, 'text'))).toEqual([])
  })
})

describe('isBinary', () => {
  test('a NUL byte early on marks a binary file', () => {
    expect(isBinary(new Uint8Array([137, 80, 78, 71, 0, 1]))).toBe(true)
    expect(isBinary(new TextEncoder().encode('plain text'))).toBe(false)
  })
})
