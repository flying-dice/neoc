import { describe, expect, test } from 'bun:test'
import { bytes, clockTime, commandLine, duration, percent, processesSummary, rate, share, started, statusLabel, whole } from './format'

describe('sizes and rates', () => {
  test('say a size in the largest unit that keeps it above 1', () => {
    expect(bytes(0)).toBe('0 B')
    expect(bytes(512)).toBe('512 B')
    expect(bytes(1024)).toBe('1.0 KB')
    expect(bytes(612 * 1024 ** 2)).toBe('612 MB')
    expect(bytes(1.2 * 1024 ** 3)).toBe('1.2 GB')
    expect(bytes(9.8 * 1024 ** 3)).toBe('9.8 GB')
    expect(bytes(2 * 1024 ** 4)).toBe('2.0 TB')
  })

  test('never go below nothing, and stop at the largest unit', () => {
    expect(bytes(-5)).toBe('0 B')
    expect(bytes(5 * 1024 ** 6)).toBe('5120 PB')
  })

  test('say a dash for a rate there is not yet', () => {
    expect(rate(undefined)).toBe('—')
    expect(rate(1.2 * 1024 ** 2)).toBe('1.2 MB/s')
    expect(rate(0)).toBe('0 B/s')
  })

  test('say a share to a tenth, or whole, or a dash', () => {
    expect(percent(38.24)).toBe('38.2%')
    expect(percent(120.07)).toBe('120.1%')
    expect(percent(undefined)).toBe('—')
    expect(whole(33.6)).toBe('34%')
    expect(whole(undefined)).toBe('—')
  })

  test('take a share of a total, and none of nothing', () => {
    expect(share(1, 4)).toBe(25)
    expect(share(5, 4)).toBe(100)
    expect(share(0, 0)).toBe(0)
    expect(share(-1, 4)).toBe(0)
  })
})

describe('time', () => {
  test('says how long in two units', () => {
    expect(duration(45)).toBe('45 s')
    expect(duration(125)).toBe('2 m 5 s')
    expect(duration(7500)).toBe('2 h 5 m')
    expect(duration(3 * 86400 + 4 * 3600 + 59)).toBe('3 d 4 h')
    expect(duration(-3)).toBe('0 s')
  })

  test('says when a process started as people say it', () => {
    const now = new Date(2025, 10, 25, 12, 0, 0)
    const at = (y: number, mo: number, d: number, h: number, mi: number): number => new Date(y, mo, d, h, mi, 0).getTime() / 1000
    expect(started(at(2025, 10, 25, 8, 14), now)).toBe('Today at 08:14')
    expect(started(at(2025, 10, 24, 22, 3), now)).toBe('Yesterday at 22:03')
    expect(started(at(2025, 10, 3, 9, 0), now)).toBe('3 Nov at 09:00')
    expect(started(at(2024, 10, 3, 9, 0), now)).toBe('3 Nov 2024')
  })

  test('says a moment as a clock', () => {
    expect(clockTime(new Date(2025, 0, 1, 7, 5, 9).getTime())).toBe('07:05:09')
  })
})

describe('words', () => {
  test('say a state, and Unknown for one that is not known', () => {
    expect(statusLabel('run')).toBe('Running')
    expect(statusLabel('uninterruptible_disk_sleep')).toBe('Waiting on Disk')
    expect(statusLabel('something_new')).toBe('Unknown')
    expect(statusLabel(undefined)).toBe('Unknown')
  })

  test('count processes', () => {
    expect(processesSummary(1)).toBe('1 Process')
    expect(processesSummary(412)).toBe('412 Processes')
    expect(processesSummary(0)).toBe('0 Processes')
  })

  test('join a command into a line', () => {
    expect(commandLine(['a', 'b c'])).toBe('a b c')
    expect(commandLine([])).toBe('')
  })
})
