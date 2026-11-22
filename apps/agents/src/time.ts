/** "5 min ago", "yesterday", or the date: when a chat was last used, from its ISO time. An unreadable time is shown as given. */
export function relativeTime(iso: string, now: number): string {
  const then = Date.parse(iso)
  if (Number.isNaN(then)) return iso
  const seconds = Math.round((then - now) / 1000)
  const format = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto', style: 'short' })
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ['second', 60],
    ['minute', 60],
    ['hour', 24],
    ['day', 7]
  ]
  let value = seconds
  for (const [unit, size] of steps) {
    if (Math.abs(value) < size)
      return unit === 'second' ? format.format(0, 'second') : format.format(value, unit)
    value = Math.round(value / size)
  }
  return new Date(then).toLocaleDateString()
}
