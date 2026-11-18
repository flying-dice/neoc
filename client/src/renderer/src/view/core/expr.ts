/**
 * The closed expression language (spec 3.2): literals, `{$ref}` and `{$fn}` calls over a fixed, pure,
 * presentation-only function set. No loops, no recursion, no user functions, no I/O, and a total cost bound
 * (`MAX_EXPR_NODES`), so an expression can neither hang nor exfiltrate and means the same on every client.
 * An unknown function, a bad argument or an over-budget expression evaluates to `undefined`; it never throws.
 *
 * Reactivity is not in here: `resolve` is where the caller reads (and so tracks) store entries.
 */
import type { Expr, Fn, Json, Lit, Ref } from '../protocol'

export const MAX_EXPR_NODES = 1_000

export const isRef = (e: unknown): e is Ref => isObject(e) && typeof e.$ref === 'string'
export const isFn = (e: unknown): e is Fn => isObject(e) && typeof e.$fn === 'string'
export const isLit = (e: unknown): e is Lit => isObject(e) && '$lit' in e

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

/** Read a path out of a value (`undefined` when any step is missing). */
export function dig(value: unknown, path: readonly string[]): unknown {
  let cur: unknown = value
  for (const seg of path) {
    if (cur === null || typeof cur !== 'object') return undefined
    cur = (cur as Record<string, unknown>)[seg]
  }
  return cur
}

/** The viewer's formatting environment (spec 3.6): locale, zone and the shared minute clock. */
export interface FormatEnv {
  locale: string
  timeZone: string
  /** `env.now`: epoch ms at minute resolution, ticked by the host window, so relative times need no messages. */
  now: number
}

type Impl = (args: unknown[], env: FormatEnv) => unknown
type Lazy = (args: Expr[], run: (e: Expr) => unknown) => unknown

const num = (v: unknown): number => (typeof v === 'number' ? v : Number(v))
const str = (v: unknown): string => (v === null || v === undefined ? '' : String(v))
const isEmptyValue = (v: unknown): boolean =>
  v === null || v === undefined || v === '' || (Array.isArray(v) && v.length === 0)

/** `YYYY-MM-DD` and `HH:mm` are floating (no zone): never shift them into the viewer's zone (spec 3.6). */
const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/
const TIME_ONLY = /^\d{2}:\d{2}(:\d{2})?$/

function toDate(v: unknown): { date: Date; floating: boolean } | null {
  if (typeof v === 'number') return { date: new Date(v), floating: false }
  if (typeof v !== 'string') return null
  if (DATE_ONLY.test(v)) return { date: new Date(`${v}T00:00:00Z`), floating: true }
  if (TIME_ONLY.test(v)) return { date: new Date(`1970-01-01T${v.length === 5 ? `${v}:00` : v}Z`), floating: true }
  const date = new Date(v)
  return Number.isNaN(date.getTime()) ? null : { date, floating: false }
}

type DateStyle = 'short' | 'medium' | 'long' | 'full'
const dateStyle = (v: unknown): DateStyle => (v === 'short' || v === 'long' || v === 'full' ? v : 'medium')

function formatDate(v: unknown, env: FormatEnv, options: (style: DateStyle) => Intl.DateTimeFormatOptions, style: unknown): string {
  const d = toDate(v)
  if (!d) return ''
  return new Intl.DateTimeFormat(env.locale, { ...options(dateStyle(style)), timeZone: d.floating ? 'UTC' : env.timeZone }).format(d.date)
}

const BYTE_UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte', 'petabyte'] as const

function formatBytes(value: unknown, env: FormatEnv): string {
  let n = num(value)
  if (!Number.isFinite(n)) return ''
  let unit = 0
  while (Math.abs(n) >= 1000 && unit < BYTE_UNITS.length - 1) {
    n /= 1000
    unit++
  }
  return new Intl.NumberFormat(env.locale, { style: 'unit', unit: BYTE_UNITS[unit], unitDisplay: 'short', maximumFractionDigits: unit === 0 ? 0 : 1 }).format(n)
}

/** Money travels as `{amount: integer minor units, currency}` (spec 3.6); a bare number is major units. */
function formatCurrency(value: unknown, currency: unknown, env: FormatEnv): string {
  let amount = value
  let code = currency
  if (isObject(value) && typeof value.amount === 'number') {
    code = value.currency
    const digits = new Intl.NumberFormat(env.locale, { style: 'currency', currency: str(code) }).resolvedOptions().maximumFractionDigits ?? 2
    amount = value.amount / 10 ** digits
  }
  if (typeof code !== 'string') return ''
  return new Intl.NumberFormat(env.locale, { style: 'currency', currency: code }).format(num(amount))
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 86_400_000],
  ['month', 30 * 86_400_000],
  ['week', 7 * 86_400_000],
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000]
]

function formatRelative(value: unknown, env: FormatEnv): string {
  const d = toDate(value)
  if (!d) return ''
  const diff = d.date.getTime() - env.now
  const rtf = new Intl.RelativeTimeFormat(env.locale, { numeric: 'auto' })
  for (const [unit, ms] of RELATIVE_UNITS) if (Math.abs(diff) >= ms) return rtf.format(Math.trunc(diff / ms), unit)
  return rtf.format(0, 'minute')
}

/** `fmt.plural(n, {one: "item", other: "items"})`; `{n}` in a form is replaced by the formatted count. */
function formatPlural(count: unknown, forms: unknown, env: FormatEnv): string {
  if (!isObject(forms)) return ''
  const n = num(count)
  const category = new Intl.PluralRules(env.locale).select(n)
  const form = forms[category] ?? forms.other
  return str(form).replaceAll('{n}', new Intl.NumberFormat(env.locale).format(n))
}

function formatList(items: unknown, style: unknown, env: FormatEnv): string {
  if (!Array.isArray(items)) return ''
  const type = style === 'or' ? 'disjunction' : 'conjunction'
  return new Intl.ListFormat(env.locale, { type }).format(items.map(str))
}

/** Strict, non-short-circuit functions. */
export const FUNCTIONS: Record<string, Impl> = {
  not: ([a]) => !a,
  eq: ([a, b]) => a === b,
  ne: ([a, b]) => a !== b,
  lt: ([a, b]) => num(a) < num(b),
  gt: ([a, b]) => num(a) > num(b),
  add: ([a, b]) => num(a) + num(b),
  sub: ([a, b]) => num(a) - num(b),
  concat: (a) => a.map(str).join(''),
  len: ([a]) => (Array.isArray(a) || typeof a === 'string' ? a.length : 0),
  isEmpty: ([a]) => isEmptyValue(a),
  upper: ([a]) => str(a).toUpperCase(),
  lower: ([a]) => str(a).toLowerCase(),
  'fmt.number': ([v, opts], env) =>
    new Intl.NumberFormat(env.locale, isObject(opts) ? (opts as Intl.NumberFormatOptions) : {}).format(num(v)),
  'fmt.currency': ([v, currency], env) => formatCurrency(v, currency, env),
  'fmt.percent': ([v, digits], env) =>
    new Intl.NumberFormat(env.locale, { style: 'percent', maximumFractionDigits: typeof digits === 'number' ? digits : 0 }).format(num(v)),
  'fmt.bytes': ([v], env) => formatBytes(v, env),
  'fmt.date': ([v, style], env) => formatDate(v, env, (s) => ({ dateStyle: s }), style),
  'fmt.time': ([v, style], env) => formatDate(v, env, (s) => ({ timeStyle: s === 'full' || s === 'long' ? 'medium' : 'short' }), style),
  'fmt.dateTime': ([v, style], env) => formatDate(v, env, (s) => ({ dateStyle: s, timeStyle: 'short' }), style),
  'fmt.relative': ([v], env) => formatRelative(v, env),
  'fmt.plural': ([n, forms], env) => formatPlural(n, forms, env),
  'fmt.list': ([items, style], env) => formatList(items, style, env)
}

/** Functions that evaluate only the arguments they need (their cost is bounded the same way). */
const LAZY: Record<string, Lazy> = {
  and: (args, run) => {
    for (const a of args) if (!run(a)) return false
    return true
  },
  or: (args, run) => {
    for (const a of args) if (run(a)) return true
    return false
  },
  if: ([c, t, f], run) => (run(c as Expr) ? run(t as Expr) : run(f as Expr)),
  coalesce: (args, run) => {
    for (const a of args) {
      const v = run(a)
      if (!isEmptyValue(v)) return v
    }
    return undefined
  }
}

export const FUNCTION_NAMES: readonly string[] = [...Object.keys(FUNCTIONS), ...Object.keys(LAZY)]

class Budget extends Error {}

/** Evaluate an expression. `resolve` maps a ref string to its current value (and is where reactivity is tracked). */
export function evaluate(expr: Expr | undefined, resolve: (ref: string) => unknown, env: FormatEnv): unknown {
  let remaining = MAX_EXPR_NODES
  const run = (e: Expr | undefined): unknown => {
    if (e === undefined) return undefined
    if (--remaining < 0) throw new Budget()
    if (isRef(e)) return resolve(e.$ref)
    if (isLit(e)) return e.$lit
    if (isFn(e)) {
      const args = e.args ?? []
      const lazy = LAZY[e.$fn]
      if (lazy) return lazy(args, run)
      const impl = FUNCTIONS[e.$fn]
      if (!impl) return undefined
      const values = args.map(run)
      try {
        return impl(values, env)
      } catch {
        return undefined // e.g. an Intl RangeError on a bad currency code: renders empty, never throws
      }
    }
    if (Array.isArray(e)) return e.map(run) as Json[]
    if (typeof e === 'object' && e !== null) return Object.fromEntries(Object.entries(e).map(([k, v]) => [k, run(v as Expr)]))
    return e
  }
  try {
    return run(expr)
  } catch (error) {
    if (error instanceof Budget) return undefined
    throw error
  }
}
