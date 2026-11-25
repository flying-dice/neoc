/** How the windows say what the backend measured: sizes, rates, shares, durations and times. */

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const

/** A size in bytes: `612 MB`, `1.2 GB`. One decimal under 10, none above. */
export function bytes(n: number): string {
  let value = Math.max(0, n)
  let unit = 0
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  const text = unit === 0 || value >= 10 ? String(Math.round(value)) : value.toFixed(1)
  return `${text} ${UNITS[unit]}`
}

/** A rate in bytes per second; a dash when there is none yet (a rate needs two readings). */
export const rate = (n: number | undefined): string => (n === undefined ? '—' : `${bytes(n)}/s`)

/** A share, to a tenth (`38.2%`); a dash when there is none yet. A process of several threads passes 100. */
export const percent = (n: number | undefined): string => (n === undefined ? '—' : `${n.toFixed(1)}%`)

/** A share, whole (`34%`), for the tiles; a dash when there is none yet. */
export const whole = (n: number | undefined): string => (n === undefined ? '—' : `${Math.round(n)}%`)

/** `used` as a share of `total`, 0 to 100; 0 for a total of nothing (a machine with no swap). */
export const share = (used: number, total: number): number => (total <= 0 ? 0 : Math.min(100, Math.max(0, (used / total) * 100)))

/** How long, in the two largest units that are not zero: `3 d 4 h`, `2 h 5 m`, `45 s`. */
export function duration(secs: number): string {
  const total = Math.max(0, Math.floor(secs))
  const d = Math.floor(total / 86400)
  const h = Math.floor((total % 86400) / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (d > 0) return `${d} d ${h} h`
  if (h > 0) return `${h} h ${m} m`
  if (m > 0) return `${m} m ${s} s`
  return `${s} s`
}

const pad = (n: number): string => String(n).padStart(2, '0')
const clock = (d: Date): string => `${pad(d.getHours())}:${pad(d.getMinutes())}`
const startOfDay = (d: Date): number => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/** When a process started, as people say it: `Today at 08:14`, `Yesterday at 22:03`, `3 Nov at 09:00`, `3 Nov 2025`. */
export function started(startSecs: number, now: Date = new Date()): string {
  const at = new Date(startSecs * 1000)
  const days = Math.round((startOfDay(now) - startOfDay(at)) / 86400000)
  if (days === 0) return `Today at ${clock(at)}`
  if (days === 1) return `Yesterday at ${clock(at)}`
  const day = `${at.getDate()} ${MONTHS[at.getMonth()]}`
  return at.getFullYear() === now.getFullYear() ? `${day} at ${clock(at)}` : `${day} ${at.getFullYear()}`
}

const STATUS: Readonly<Record<string, string>> = {
  run: 'Running',
  sleep: 'Sleeping',
  idle: 'Idle',
  stop: 'Stopped',
  zombie: 'Zombie',
  tracing: 'Being Traced',
  dead: 'Dead',
  wakekill: 'Waking',
  waking: 'Waking',
  parked: 'Parked',
  lock_blocked: 'Waiting on a Lock',
  uninterruptible_disk_sleep: 'Waiting on Disk',
  suspended: 'Suspended'
}

/** What state a process is in, in words; a platform's state that is not known is `Unknown`. */
export const statusLabel = (status: string | undefined): string => (status === undefined ? 'Unknown' : (STATUS[status] ?? 'Unknown'))

/** How many processes there are, as a section header. */
export const processesSummary = (count: number): string => (count === 1 ? '1 Process' : `${count} Processes`)

/** The command of a process as one line. */
export const commandLine = (command: readonly string[]): string => command.join(' ')

/** `HH:MM:SS` of a moment, for "Updated …". */
export const clockTime = (at: number): string => {
  const d = new Date(at)
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}
