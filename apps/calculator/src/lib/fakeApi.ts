/**
 * A stand-in for the Calculator's backend, for stories and unit tests: `press` answered in the page, with no host. It
 * follows the keypad's rules (server/keypad.luau) closely enough to do sums on: digits and the decimal point, the four
 * operators with their precedence, =, AC and C, ±, %, backspace, and the failure of a division by zero. The real
 * arithmetic (rounding, exponents, the caption's ellipsis) is the backend's, and the journeys exercise it.
 */
import type { Keypad, Screen } from '@calculator/rpc'
import type { CalculatorRpc } from './calculator.svelte'
import type { Key, Operator } from './keys'

/** A sum that cannot be worked out: the display shows Error and the caption says why. */
class SumError extends Error {}

const MAX_DIGITS = 9
const SYMBOL: Readonly<Record<Operator, string>> = { '+': '+', '-': '−', '*': '×', '/': '÷' }
const OPERATORS: ReadonlySet<string> = new Set(Object.keys(SYMBOL))
const isOperator = (key: Key): key is Operator => OPERATORS.has(key)

const initial = (): Keypad => ({
  entry: '0',
  value: 0,
  numbers: [],
  ops: [],
  waiting: false,
  fresh: true,
  caption: '',
  err: false
})

function combine(a: number, op: Operator, b: number): number {
  if (op === '/' && b === 0) throw new SumError("Can't divide by zero")
  const result = op === '+' ? a + b : op === '-' ? a - b : op === '*' ? a * b : a / b
  return Number(result.toPrecision(12))
}

/** `numbers[i]` then `ops[i]`: × and ÷ first, then + and −, left to right. */
function total(numbers: readonly number[], ops: readonly Operator[]): number {
  const sums: number[] = [numbers[0] ?? 0]
  const signs: Operator[] = []
  numbers.slice(1).forEach((n, i) => {
    const op = ops[i] ?? '+'
    if (op === '*' || op === '/') sums[sums.length - 1] = combine(sums[sums.length - 1] ?? 0, op, n)
    else {
      sums.push(n)
      signs.push(op)
    }
  })
  return sums.slice(1).reduce((acc, n, i) => combine(acc, signs[i] ?? '+', n), sums[0] ?? 0)
}

/** What the last operator completes: `2 + 3 ×` is still 3, `2 × 3 +` is 6. */
function progress(numbers: readonly number[], ops: readonly Operator[]): number {
  const last = ops[ops.length - 1]
  if (last === '+' || last === '-') return total(numbers, ops)
  let start = numbers.length - 1
  while (start > 0 && (ops[start - 1] === '*' || ops[start - 1] === '/')) start--
  return total(numbers.slice(start), ops.slice(start))
}

const group = (digits: string): string => digits.replace(/\B(?=(\d{3})+$)/g, ',')

function formatEntry(entry: string): string {
  const [, sign = '', integer = '', rest = ''] = /^(-?)(\d*)(.*)$/.exec(entry) ?? []
  return sign + group(integer === '' ? '0' : integer) + rest
}

function format(value: number): string {
  const rounded = Number(value.toPrecision(MAX_DIGITS))
  if (Math.abs(rounded) >= 1e9 || (rounded !== 0 && Math.abs(rounded) < 1e-6)) {
    return rounded.toExponential().replace('e+', 'e')
  }
  return formatEntry(String(rounded))
}

function describe(numbers: readonly number[], ops: readonly Operator[]): string {
  return numbers.flatMap((n, i) => [format(n), ...(ops[i] ? [SYMBOL[ops[i]]] : [])]).join(' ')
}

const current = (s: Keypad): number => (s.fresh ? s.value : Number(s.entry))

function clear(s: Keypad): Keypad {
  if (s.entry !== '0' && !s.fresh) return { ...s, entry: '0', waiting: s.numbers.length > 0, err: false, value: 0, fresh: true }
  return initial()
}

function digit(s: Keypad, d: string): Keypad {
  const typing = { ...s, waiting: false, fresh: false }
  if (s.fresh) return { ...typing, entry: d === '.' ? '0.' : d }
  if (d === '.') return s.entry.includes('.') ? typing : { ...typing, entry: `${s.entry}.` }
  if (s.entry.replace(/[-.]/g, '').length >= MAX_DIGITS) return typing
  return { ...typing, entry: s.entry === '0' ? d : s.entry + d }
}

function operator(s: Keypad, op: Operator): Keypad {
  const replacing = s.waiting && s.ops.length > 0
  const numbers = replacing ? s.numbers : [...s.numbers, current(s)]
  const ops = replacing ? [...s.ops.slice(0, -1), op] : [...s.ops, op]
  return { ...s, numbers, ops, waiting: true, value: progress(numbers, ops), fresh: true, caption: describe(numbers, ops) }
}

function equals(s: Keypad): Keypad {
  if (s.numbers.length === 0) return s
  const numbers = [...s.numbers, current(s)]
  return { ...s, numbers: [], ops: [], waiting: false, value: total(numbers, s.ops), fresh: true, caption: describe(numbers, s.ops) }
}

function negate(s: Keypad): Keypad {
  if (s.fresh) return { ...s, waiting: false, value: s.value === 0 ? 0 : -s.value }
  const entry = s.entry.startsWith('-') ? s.entry.slice(1) : s.entry === '0' ? s.entry : `-${s.entry}`
  return { ...s, waiting: false, entry }
}

function backspace(s: Keypad): Keypad {
  if (s.fresh) return s
  const rest = s.entry.slice(0, -1)
  return { ...s, entry: rest === '' || rest === '-' || rest === '-0' ? '0' : rest }
}

function apply(s: Keypad, key: Key): Keypad {
  if (isOperator(key)) return operator(s, key)
  switch (key) {
    case '=':
      return equals(s)
    case 'clear':
      return clear(s)
    case 'negate':
      return negate(s)
    case 'percent':
      return { ...s, value: combine(current(s), '/', 100), fresh: true, waiting: false }
    case 'backspace':
      return backspace(s)
    default:
      return digit(s, key)
  }
}

function screen(s: Keypad): Screen {
  const typing = s.entry !== '0' && !s.fresh
  const pending = s.waiting ? s.ops[s.ops.length - 1] : undefined
  return {
    state: s,
    display: s.err ? 'Error' : s.fresh ? format(s.value) : formatEntry(s.entry),
    caption: s.caption,
    clear: typing ? 'C' : 'AC',
    ...(pending ? { pending } : {}),
    ...(s.err ? { error: s.caption } : {})
  }
}

function press(given: Keypad, key: Key): Screen {
  let s = given
  if (s.err) {
    // AC gets out of an error, and so does a digit, which starts a new sum; the other keys have nothing to work on
    if (/^[\d.]$/.test(key)) s = initial()
    else if (key !== 'clear') return screen(s)
  }
  try {
    return screen(apply(s, key))
  } catch (e) {
    if (!(e instanceof SumError)) throw e
    return screen({ ...initial(), err: true, caption: e.message })
  }
}

/** A backend in the page: every key is answered, on the next turn of the event loop as a real answer would be. */
export function createFakeApi(): CalculatorRpc {
  return { press: async ({ key, state }) => press(state ?? initial(), key) }
}

/** A backend that cannot be reached: every key is refused with `message`, as the host or a lost connection would. */
export function createFailingApi(message: string): CalculatorRpc {
  return { press: async () => Promise.reject(new Error(message)) }
}
