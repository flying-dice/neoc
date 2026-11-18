/**
 * DatePicker value handling. The wire values are strings: `YYYY-MM-DD` (date), `HH:mm` (time) or an ISO-8601 UTC
 * instant (dateTime, `2026-11-18T09:30:00Z`). The picker edits *local* wall-clock parts in the viewer's time zone;
 * this module converts between the two, clamps to `min`/`max`, steps minutes and builds the month grid. No
 * `Date` parsing of free text: every input is matched against a strict pattern first.
 */

export type DateKind = 'date' | 'time' | 'dateTime'

export interface Parts {
  year: number
  month: number // 1..12
  day: number
  hour: number
  minute: number
}

const DATE = /^(\d{4})-(\d{2})-(\d{2})$/
const TIME = /^(\d{2}):(\d{2})$/
const INSTANT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?(Z|[+-]\d{2}:\d{2})$/

const pad = (n: number, width = 2): string => String(n).padStart(width, '0')

export const daysInMonth = (year: number, month: number): number =>
  new Date(Date.UTC(year, month, 0)).getUTCDate()

const validDate = (y: number, m: number, d: number): boolean =>
  m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m)

/** Offset of `timeZone` from UTC at `at`, in minutes (positive east). */
export function zoneOffsetMinutes(at: Date, timeZone: string): number {
  const f = new Intl.DateTimeFormat('en-US', {
    timeZone,
    hourCycle: 'h23',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
  const p = Object.fromEntries(f.formatToParts(at).map((x) => [x.type, Number(x.value)]))
  const asUtc = Date.UTC(p.year!, p.month! - 1, p.day!, p.hour!, p.minute!, p.second!)
  return Math.round((asUtc - Math.floor(at.getTime() / 1000) * 1000) / 60000)
}

/** Wire value to local wall-clock parts (null when it does not match the kind's pattern or is not a real date). */
export function parseValue(kind: DateKind, value: unknown, timeZone: string): Parts | null {
  if (typeof value !== 'string') return null
  if (kind === 'date') {
    const m = DATE.exec(value)
    if (!m) return null
    const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])]
    return validDate(y, mo, d) ? { year: y, month: mo, day: d, hour: 0, minute: 0 } : null
  }
  if (kind === 'time') {
    const m = TIME.exec(value)
    if (!m) return null
    const [h, mi] = [Number(m[1]), Number(m[2])]
    return h < 24 && mi < 60 ? { year: 1970, month: 1, day: 1, hour: h, minute: mi } : null
  }
  const m = INSTANT.exec(value)
  if (!m) return null
  const [y, mo, d, h, mi] = [m[1], m[2], m[3], m[4], m[5]].map(Number)
  if (!validDate(y!, mo!, d!) || h! > 23 || mi! > 59) return null
  const at = new Date(value)
  if (Number.isNaN(at.getTime())) return null
  const local = new Date(at.getTime() + zoneOffsetMinutes(at, timeZone) * 60000)
  return {
    year: local.getUTCFullYear(),
    month: local.getUTCMonth() + 1,
    day: local.getUTCDate(),
    hour: local.getUTCHours(),
    minute: local.getUTCMinutes()
  }
}

/** Local wall-clock parts to the wire value. A dateTime is written as a UTC instant at minute precision. */
export function formatValue(kind: DateKind, p: Parts, timeZone: string): string {
  if (kind === 'date') return `${pad(p.year, 4)}-${pad(p.month)}-${pad(p.day)}`
  if (kind === 'time') return `${pad(p.hour)}:${pad(p.minute)}`
  // Wall clock in `timeZone` -> UTC: guess the offset at the wall-clock-as-UTC instant, then correct once for a DST edge.
  const wall = Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute)
  let utc = wall - zoneOffsetMinutes(new Date(wall), timeZone) * 60000
  utc = wall - zoneOffsetMinutes(new Date(utc), timeZone) * 60000
  const d = new Date(utc)
  return `${pad(d.getUTCFullYear(), 4)}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:00Z`
}

/** Comparable number for the parts under a kind: the date, the time, or both. */
const rank = (kind: DateKind, p: Parts): number =>
  kind === 'time'
    ? p.hour * 60 + p.minute
    : kind === 'date'
      ? p.year * 10000 + p.month * 100 + p.day
      : Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute)

/** Clamp parts into `[min, max]` (wire values; either may be absent or invalid, which is ignored). */
export function clampParts(
  kind: DateKind,
  p: Parts,
  min: unknown,
  max: unknown,
  timeZone: string
): Parts {
  const lo = parseValue(kind, min, timeZone)
  const hi = parseValue(kind, max, timeZone)
  if (lo && rank(kind, p) < rank(kind, lo)) return lo
  if (hi && rank(kind, p) > rank(kind, hi)) return hi
  return p
}

/** Round minutes to a multiple of `step` (minutes, 1..60); an invalid step is 1. */
export function stepMinutes(minute: number, step: unknown): number {
  const s = typeof step === 'number' && Number.isInteger(step) && step >= 1 && step <= 60 ? step : 1
  return (Math.round(minute / s) * s) % 60
}

/** Add whole days to date parts, rolling the month and year. */
export function addDays(p: Parts, days: number): Parts {
  const d = new Date(Date.UTC(p.year, p.month - 1, p.day + days))
  return { ...p, year: d.getUTCFullYear(), month: d.getUTCMonth() + 1, day: d.getUTCDate() }
}

/** Add whole months, keeping the day when it exists and the last day of the month otherwise. */
export function addMonths(p: Parts, months: number): Parts {
  const index = p.year * 12 + (p.month - 1) + months
  const year = Math.floor(index / 12)
  const month = (index % 12) + 1
  return { ...p, year, month, day: Math.min(p.day, daysInMonth(year, month)) }
}

export interface Cell {
  year: number
  month: number
  day: number
  /** In the shown month (false for the leading and trailing days of the neighbours). */
  inMonth: boolean
}

/** The weeks of a month, Monday or Sunday first, always full rows (5 or 6). */
export function monthGrid(year: number, month: number, weekStart: 0 | 1 = 1): Cell[][] {
  const first = new Date(Date.UTC(year, month - 1, 1)).getUTCDay() // 0 = Sunday
  const lead = (first - weekStart + 7) % 7
  const total = Math.ceil((lead + daysInMonth(year, month)) / 7) * 7
  const weeks: Cell[][] = []
  for (let i = 0; i < total; i++) {
    const d = new Date(Date.UTC(year, month - 1, 1 - lead + i))
    const cell: Cell = {
      year: d.getUTCFullYear(),
      month: d.getUTCMonth() + 1,
      day: d.getUTCDate(),
      inMonth: d.getUTCMonth() + 1 === month
    }
    if (i % 7 === 0) weeks.push([])
    weeks[weeks.length - 1]!.push(cell)
  }
  return weeks
}

export const sameDay = (a: Pick<Parts, 'year' | 'month' | 'day'>, b: typeof a): boolean =>
  a.year === b.year && a.month === b.month && a.day === b.day

/** Is a day outside `[min, max]` (date granularity)? Disabled days cannot be picked. */
export function dayDisabled(
  cell: Pick<Parts, 'year' | 'month' | 'day'>,
  min: unknown,
  max: unknown,
  timeZone: string
): boolean {
  const key = cell.year * 10000 + cell.month * 100 + cell.day
  const lo = parseValue('date', typeof min === 'string' ? min.slice(0, 10) : min, timeZone)
  const hi = parseValue('date', typeof max === 'string' ? max.slice(0, 10) : max, timeZone)
  return (!!lo && key < rank('date', lo)) || (!!hi && key > rank('date', hi))
}

/** Native `<input>` string for a kind (`date`, `time`, `datetime-local`) from parts, and back. */
export function toNative(kind: DateKind, p: Parts): string {
  if (kind === 'date') return formatValue('date', p, 'UTC')
  if (kind === 'time') return formatValue('time', p, 'UTC')
  return `${formatValue('date', p, 'UTC')}T${formatValue('time', p, 'UTC')}`
}

export function fromNative(kind: DateKind, value: string): Parts | null {
  if (kind === 'date') return parseValue('date', value, 'UTC')
  if (kind === 'time') return parseValue('time', value, 'UTC')
  const [d, t] = value.split('T')
  const date = parseValue('date', d, 'UTC')
  const time = parseValue('time', t?.slice(0, 5), 'UTC')
  return date && time ? { ...date, hour: time.hour, minute: time.minute } : null
}

export const nativeType = (kind: DateKind): 'date' | 'time' | 'datetime-local' =>
  kind === 'date' ? 'date' : kind === 'time' ? 'time' : 'datetime-local'
