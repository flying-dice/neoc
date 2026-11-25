import type { PressParams, Screen } from '@calculator/rpc'

/** A key of the keypad, as `press` takes it. */
export type Key = PressParams['key']
/** An operator that can be waiting for its second number. */
export type Operator = NonNullable<Screen['pending']>
/** What the clear key says: all clear, or clear the entry being typed. */
export type ClearLabel = Screen['clear']

export type Face = 'digit' | 'function' | 'operator'

export interface KeyDef {
  readonly key: Key
  /** the key's part of its test id (`calculator-key-<id>`) */
  readonly id: string
  readonly label: string
  /** the accessible name, when the label is a glyph */
  readonly name?: string
  readonly face: Face
}

const digit = (d: Key): KeyDef => ({ key: d, id: d, label: d, face: 'digit' })
const op = (key: Key, id: string, label: string, name: string): KeyDef => ({ key, id, label, name, face: 'operator' })

/** The iPadOS keypad, row by row; the clear key's label changes (AC or C) and is drawn from the screen. */
export const KEYS: readonly KeyDef[] = [
  { key: 'clear', id: 'clear', label: 'AC', face: 'function' },
  { key: 'negate', id: 'negate', label: '±', name: 'Change sign', face: 'function' },
  { key: 'percent', id: 'percent', label: '%', name: 'Percent', face: 'function' },
  op('/', 'div', '÷', 'Divided by'),
  digit('7'),
  digit('8'),
  digit('9'),
  op('*', 'mul', '×', 'Times'),
  digit('4'),
  digit('5'),
  digit('6'),
  op('-', 'sub', '−', 'Minus'),
  digit('1'),
  digit('2'),
  digit('3'),
  op('+', 'add', '+', 'Plus'),
  digit('0'),
  { key: '.', id: 'dot', label: '.', name: 'Decimal point', face: 'digit' },
  op('=', 'eq', '=', 'Equals')
]

/** A palette token per face: the kit picks a text colour with 4.5:1 on it. */
export const FILL: Readonly<Record<Face, string>> = { digit: 'gray4', function: 'gray2', operator: 'orange' }

/** The keyboard: digits, operators, = or Enter, Backspace, Escape, %. */
const TYPED: ReadonlyMap<string, Key> = new Map<string, Key>([
  ...(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] as const).map((d): [string, Key] => [d, d]),
  ['+', '+'],
  ['-', '-'],
  ['*', '*'],
  ['x', '*'],
  ['/', '/'],
  ['=', '='],
  ['Enter', '='],
  ['%', 'percent'],
  ['Backspace', 'backspace'],
  ['Escape', 'clear'],
  ['.', '.'],
  [',', '.']
])

/** The keypad key a keyboard key stands for (`KeyboardEvent.key`), if any. */
export function keyTyped(name: string): Key | undefined {
  return TYPED.get(name)
}
