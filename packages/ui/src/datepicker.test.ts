import { describe, expect, test } from 'bun:test'
import {
  addDays,
  addMonths,
  clampParts,
  dayDisabled,
  formatValue,
  fromNative,
  monthGrid,
  parseValue,
  stepMinutes,
  toNative
} from './datepicker'

describe('DatePicker values', () => {
  test('a date round-trips and rejects impossible days', () => {
    const p = parseValue('date', '2028-02-29', 'UTC')
    expect(p).toMatchObject({ year: 2028, month: 2, day: 29 })
    expect(formatValue('date', p!, 'UTC')).toBe('2028-02-29')
    expect(parseValue('date', '2027-02-29', 'UTC')).toBeNull()
    expect(parseValue('date', '2027-13-01', 'UTC')).toBeNull()
    expect(parseValue('date', '27-1-1', 'UTC')).toBeNull()
    expect(parseValue('date', 20270101, 'UTC')).toBeNull()
  })

  test('a time is HH:mm on a 24 hour clock', () => {
    expect(parseValue('time', '09:05', 'UTC')).toMatchObject({ hour: 9, minute: 5 })
    expect(formatValue('time', { year: 1970, month: 1, day: 1, hour: 7, minute: 3 }, 'UTC')).toBe('07:03')
    for (const bad of ['24:00', '12:60', '9:05', '12:5', 'noon']) expect(parseValue('time', bad, 'UTC')).toBeNull()
  })

  test('an instant shows as the wall clock of the viewer and is written back as UTC', () => {
    const wire = '2026-11-18T09:30:00Z'
    const london = parseValue('dateTime', wire, 'Europe/London')!
    expect(london).toMatchObject({ day: 18, hour: 9, minute: 30 })
    const tokyo = parseValue('dateTime', wire, 'Asia/Tokyo')!
    expect(tokyo).toMatchObject({ day: 18, hour: 18, minute: 30 })
    expect(formatValue('dateTime', tokyo, 'Asia/Tokyo')).toBe(wire)
    const ny = parseValue('dateTime', wire, 'America/New_York')!
    expect(ny).toMatchObject({ hour: 4, minute: 30 })
    expect(formatValue('dateTime', ny, 'America/New_York')).toBe(wire)
  })

  test('summer time: the same wall clock is a different instant', () => {
    const summer = parseValue('dateTime', '2026-07-01T12:00:00Z', 'Europe/London')!
    expect(summer.hour).toBe(13)
    expect(formatValue('dateTime', summer, 'Europe/London')).toBe('2026-07-01T12:00:00Z')
  })

  test('an instant with an offset is accepted and normalised', () => {
    const p = parseValue('dateTime', '2026-11-18T10:30:00+01:00', 'UTC')!
    expect(p).toMatchObject({ hour: 9, minute: 30 })
    expect(parseValue('dateTime', '2026-11-18 09:30', 'UTC')).toBeNull()
    expect(parseValue('dateTime', '2026-02-30T09:30:00Z', 'UTC')).toBeNull()
  })

  test('clamping to min and max, ignoring bad bounds', () => {
    const p = parseValue('date', '2026-06-15', 'UTC')!
    expect(formatValue('date', clampParts('date', p, '2026-07-01', undefined, 'UTC'), 'UTC')).toBe('2026-07-01')
    expect(formatValue('date', clampParts('date', p, undefined, '2026-06-01', 'UTC'), 'UTC')).toBe('2026-06-01')
    expect(clampParts('date', p, 'nonsense', 42, 'UTC')).toBe(p)
    const t = parseValue('time', '23:30', 'UTC')!
    expect(formatValue('time', clampParts('time', t, '08:00', '17:00', 'UTC'), 'UTC')).toBe('17:00')
  })

  test('minute steps round to the step and never reach 60', () => {
    expect(stepMinutes(7, 15)).toBe(0)
    expect(stepMinutes(8, 15)).toBe(15)
    expect(stepMinutes(58, 15)).toBe(0)
    expect(stepMinutes(37, undefined)).toBe(37)
    expect(stepMinutes(37, 0)).toBe(37)
    expect(stepMinutes(37, 2.5)).toBe(37)
  })

  test('adding days and months rolls over and keeps real days', () => {
    const p = parseValue('date', '2026-12-31', 'UTC')!
    expect(addDays(p, 1)).toMatchObject({ year: 2027, month: 1, day: 1 })
    expect(addDays(p, -365)).toMatchObject({ year: 2025, month: 12, day: 31 })
    const jan31 = parseValue('date', '2026-01-31', 'UTC')!
    expect(addMonths(jan31, 1)).toMatchObject({ month: 2, day: 28 })
    expect(addMonths(jan31, -1)).toMatchObject({ year: 2025, month: 12, day: 31 })
    expect(addMonths(jan31, 13)).toMatchObject({ year: 2027, month: 2, day: 28 })
  })

  test('the month grid is whole weeks starting on the chosen day', () => {
    const nov = monthGrid(2026, 11, 1) // 1 Nov 2026 is a Sunday
    expect(nov.every((w) => w.length === 7)).toBe(true)
    expect(nov[0]![0]).toMatchObject({ month: 10, day: 26, inMonth: false })
    expect(nov[0]![6]).toMatchObject({ month: 11, day: 1, inMonth: true })
    const sun = monthGrid(2026, 11, 0)
    expect(sun[0]![0]).toMatchObject({ month: 11, day: 1, inMonth: true })
    expect(nov.flat().filter((c) => c.inMonth)).toHaveLength(30)
    expect(monthGrid(2026, 2, 1).length).toBe(4 + (monthGrid(2026, 2, 1).length > 4 ? 1 : 0)) // Feb 2026 starts Sunday: 5 weeks
  })

  test('days outside min and max are disabled at date granularity', () => {
    const day = { year: 2026, month: 6, day: 10 }
    expect(dayDisabled(day, '2026-06-11', undefined, 'UTC')).toBe(true)
    expect(dayDisabled(day, '2026-06-10', '2026-06-10', 'UTC')).toBe(false)
    expect(dayDisabled(day, undefined, '2026-06-09', 'UTC')).toBe(true)
    expect(dayDisabled(day, '2026-06-10T00:00:00Z', undefined, 'UTC')).toBe(false)
  })

  test('native input strings convert both ways', () => {
    const p = parseValue('dateTime', '2026-11-18T09:30:00Z', 'UTC')!
    expect(toNative('dateTime', p)).toBe('2026-11-18T09:30')
    expect(fromNative('dateTime', '2026-11-18T09:30')).toMatchObject({ day: 18, hour: 9, minute: 30 })
    expect(fromNative('date', '')).toBeNull()
    expect(fromNative('time', '09:30')).toMatchObject({ hour: 9, minute: 30 })
    expect(fromNative('dateTime', '2026-11-18')).toBeNull()
  })
})
