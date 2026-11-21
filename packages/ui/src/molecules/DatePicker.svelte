<!--
  @molecule DatePicker — a date, a time or both. `compact` is a pill of native inputs (the platform's own picker,
  keyboard and spoken UI); `graphical` is a month calendar: a grid of day buttons with roving focus (arrows move a
  day or a week, PageUp/PageDown a month, Home/End the week's ends, Enter or Space picks) and, for date and
  time, a time input below. Values are the wire strings (`YYYY-MM-DD`, `HH:mm`, ISO-8601 UTC), shown in the
  viewer's time zone; `min` and `max` disable days and clamp times. Atoms: Icon.
  Test ids: `testid` on the group, `${testid}-input` / `${testid}-time` (native inputs), `${testid}-prev`,
  `${testid}-next`, `${testid}-day-<YYYY-MM-DD>`.
-->
<script lang="ts">
  import { describe } from '../aria'
  import { tick } from 'svelte'
  import Icon from '../atoms/Icon.svelte'
  import {
    addDays,
    addMonths,
    clampParts,
    dayDisabled,
    formatValue,
    fromNative,
    monthGrid,
    nativeType,
    parseValue,
    sameDay,
    stepMinutes,
    toNative,
    type DateKind,
    type Parts
  } from '../datepicker'

  interface Props {
    testid: string
    /** The wire value; empty or invalid shows an empty picker. */
    value?: string
    kind?: DateKind
    style?: 'compact' | 'graphical'
    min?: string
    max?: string
    /** Minute step for the time input (1..60). */
    step?: number
    /** Accessible name. */
    label: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    disabled?: boolean
    /** IANA zone the value is shown in (default: the viewer's). */
    timeZone?: string
    locale?: string
    /** 0 = Sunday, 1 = Monday. */
    weekStart?: 0 | 1
    onchange?: (value: string) => void
  }

  let {
    testid,
    value = $bindable(''),
    kind = 'date',
    style = 'compact',
    min,
    max,
    step,
    label,
    'aria-description': description,
    disabled = false,
    timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
    locale,
    weekStart = 1,
    onchange
  }: Props = $props()

  const parts = $derived(parseValue(kind, value, timeZone))
  const today: Parts = $derived.by(() => {
    const now = new Date()
    return (
      parseValue('dateTime', now.toISOString().replace(/\.\d+Z$/, 'Z'), timeZone) ?? {
        year: now.getUTCFullYear(),
        month: now.getUTCMonth() + 1,
        day: now.getUTCDate(),
        hour: 0,
        minute: 0
      }
    )
  })

  function commit(next: Parts): void {
    const stepped = kind === 'date' ? next : { ...next, minute: stepMinutes(next.minute, step) }
    const clamped = clampParts(kind, stepped, min, max, timeZone)
    const wire = formatValue(kind, clamped, timeZone)
    if (wire === value) return
    value = wire
    onchange?.(wire)
  }

  // ── compact: native inputs ────────────────────────────────────────────
  const nativeValue = $derived(parts ? toNative(kind, parts) : '')
  function onnative(e: Event): void {
    const next = fromNative(kind, (e.currentTarget as HTMLInputElement).value)
    if (next) commit(next)
  }
  const nativeBound = (b: string | undefined): string | undefined => {
    const p = parseValue(kind, b, timeZone)
    return p ? toNative(kind, p) : undefined
  }

  // ── graphical: the month grid ─────────────────────────────────────────
  let shown = $state<{ year: number; month: number }>({ year: 0, month: 0 })
  let focusDay: Parts | null = $state(null)
  let grid: HTMLElement | undefined = $state()

  // The calendar opens on the value's month (or today's) and follows the value when it changes from outside.
  $effect(() => {
    const base = parts ?? today
    shown = { year: base.year, month: base.month }
    focusDay = null
  })

  const weeks = $derived(monthGrid(shown.year, shown.month, weekStart))
  const monthLabel = $derived(
    new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(
      new Date(Date.UTC(shown.year, shown.month - 1, 1))
    )
  )
  const weekdays = $derived(
    Array.from({ length: 7 }, (_, i) => {
      const d = new Date(Date.UTC(2024, 0, 7 + weekStart + i)) // 7 Jan 2024 is a Sunday
      return {
        short: new Intl.DateTimeFormat(locale, { weekday: 'narrow', timeZone: 'UTC' }).format(d),
        long: new Intl.DateTimeFormat(locale, { weekday: 'long', timeZone: 'UTC' }).format(d)
      }
    })
  )
  const key = (c: { year: number; month: number; day: number }): string =>
    `${String(c.year).padStart(4, '0')}-${String(c.month).padStart(2, '0')}-${String(c.day).padStart(2, '0')}`

  /** The one tab stop: the focused day, else the picked day if it is shown, else the 1st (or today). */
  const stop = $derived.by(() => {
    if (focusDay) return focusDay
    if (parts && parts.year === shown.year && parts.month === shown.month) return parts
    if (today.year === shown.year && today.month === shown.month) return today
    return { year: shown.year, month: shown.month, day: 1, hour: 0, minute: 0 }
  })

  function showMonth(delta: number): void {
    const next = addMonths({ ...stop, year: shown.year, month: shown.month, day: 1 }, delta)
    shown = { year: next.year, month: next.month }
    focusDay = null
  }

  async function moveFocus(to: Parts): Promise<void> {
    focusDay = to
    shown = { year: to.year, month: to.month }
    await tick()
    grid?.querySelector<HTMLElement>(`[data-day="${key(to)}"]`)?.focus()
  }

  function pick(c: { year: number; month: number; day: number }): void {
    if (disabled || dayDisabled(c, min, max, timeZone)) return
    const base = parts ?? today
    commit({ ...base, year: c.year, month: c.month, day: c.day, hour: kind === 'dateTime' ? (parts?.hour ?? 9) : 0, minute: kind === 'dateTime' ? (parts?.minute ?? 0) : 0 })
    focusDay = { ...base, year: c.year, month: c.month, day: c.day }
  }

  function onGridKey(e: KeyboardEvent): void {
    const from = stop
    const moves: Record<string, Parts> = {
      ArrowLeft: addDays(from, -1),
      ArrowRight: addDays(from, 1),
      ArrowUp: addDays(from, -7),
      ArrowDown: addDays(from, 7),
      PageUp: addMonths(from, -1),
      PageDown: addMonths(from, 1),
      Home: addDays(from, -((new Date(Date.UTC(from.year, from.month - 1, from.day)).getUTCDay() - weekStart + 7) % 7)),
      End: addDays(from, 6 - ((new Date(Date.UTC(from.year, from.month - 1, from.day)).getUTCDay() - weekStart + 7) % 7))
    }
    const to = moves[e.key]
    if (!to) return
    e.preventDefault()
    void moveFocus(to)
  }

  const showTime = $derived(kind === 'dateTime' || kind === 'time')
  const timeValue = $derived(parts ? toNative('time', parts) : '')
  function ontime(e: Event): void {
    const t = fromNative('time', (e.currentTarget as HTMLInputElement).value)
    if (!t) return
    commit({ ...(parts ?? today), hour: t.hour, minute: t.minute })
  }
</script>

{#if style === 'graphical' && kind !== 'time'}
  <div class="ios-datepicker graphical" class:disabled role="group" aria-label={label} {...describe(description)} data-testid={testid}>
    <div class="head">
      <h3 class="month" aria-live="polite">{monthLabel}</h3>
      <button type="button" class="nav" data-testid="{testid}-prev" aria-label="Previous month" {disabled} onclick={() => showMonth(-1)}>
        <Icon name="chevron.left" size={18} weight={2.6} />
      </button>
      <button type="button" class="nav" data-testid="{testid}-next" aria-label="Next month" {disabled} onclick={() => showMonth(1)}>
        <Icon name="chevron.right" size={18} weight={2.6} />
      </button>
    </div>
    <div class="grid" role="grid" data-testid="{testid}-grid" aria-label={monthLabel} bind:this={grid} tabindex="-1" onkeydown={onGridKey}>
      <div class="row" role="row">
        {#each weekdays as d (d.long)}
          <span class="dow" role="columnheader" aria-label={d.long}>{d.short}</span>
        {/each}
      </div>
      {#each weeks as week (key(week[0]!))}
        <div class="row" role="row">
          {#each week as cell (key(cell))}
            {@const picked = parts ? sameDay(parts, cell) : false}
            {@const off = dayDisabled(cell, min, max, timeZone)}
            <span class="cell" role="gridcell" aria-selected={picked}>
              <button
                type="button"
                class="day"
                class:picked
                class:out={!cell.inMonth}
                class:today={sameDay(today, cell)}
                data-day={key(cell)}
                data-testid="{testid}-day-{key(cell)}"
                tabindex={sameDay(stop, cell) ? 0 : -1}
                aria-label={new Intl.DateTimeFormat(locale, { dateStyle: 'full', timeZone: 'UTC' }).format(new Date(Date.UTC(cell.year, cell.month - 1, cell.day)))}
                aria-current={sameDay(today, cell) ? 'date' : undefined}
                disabled={disabled || off}
                onclick={() => pick(cell)}
              >
                {cell.day}
              </button>
            </span>
          {/each}
        </div>
      {/each}
    </div>
    {#if showTime}
      <label class="time-row">
        <span class="time-label">Time</span>
        <input
          class="native"
          type="time"
          step={(step ?? 1) * 60}
          data-testid="{testid}-time"
          aria-label="{label}, time"
          value={timeValue}
          {disabled}
          onchange={ontime}
        />
      </label>
    {/if}
  </div>
{:else}
  <div class="ios-datepicker compact" class:disabled role="group" aria-label={label} {...describe(description)} data-testid={testid}>
    <input
      class="native pill"
      type={nativeType(kind)}
      data-testid="{testid}-input"
      aria-label={label}
      value={nativeValue}
      min={nativeBound(min)}
      max={nativeBound(max)}
      step={showTime ? (step ?? 1) * 60 : undefined}
      {disabled}
      onchange={onnative}
    />
  </div>
{/if}

<style>
  .ios-datepicker {
    display: inline-flex;
    flex-direction: column;
    max-width: 100%;
    color: var(--label);
  }
  .disabled {
    opacity: 0.4;
  }
  .native {
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    color-scheme: inherit;
    min-height: 36px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--fill-tertiary);
    max-width: 100%;
    font-variant-numeric: tabular-nums;
  }
  .native:focus-visible {
    outline-offset: 0;
  }
  .graphical {
    width: min(100%, 320px);
    padding: 8px 8px 12px;
    background: var(--bg-grouped-elevated-secondary);
    border-radius: var(--radius-lg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 4px 6px 8px;
  }
  .month {
    flex: 1;
    margin: 0;
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
  }
  .nav {
    appearance: none;
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    color: var(--tint);
    cursor: pointer;
  }
  .nav:active:not(:disabled) {
    background: var(--fill-secondary);
  }
  .grid {
    display: flex;
    flex-direction: column;
    outline: 0;
  }
  .row {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
  }
  .dow {
    display: grid;
    place-items: center;
    height: 28px;
    font: var(--type-caption1);
    font-weight: 600;
    color: var(--label-secondary);
  }
  .cell {
    display: grid;
    place-items: center;
  }
  .day {
    appearance: none;
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
    width: 100%;
    max-width: 44px;
    aspect-ratio: 1;
    min-height: 36px;
    border-radius: 50%;
    font: var(--type-body);
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
    color: var(--label);
    cursor: pointer;
  }
  .day.out {
    color: var(--label-secondary);
  }
  .day.today {
    color: var(--tint);
    font-weight: 600;
  }
  .day.picked {
    background: var(--tint-fill);
    color: #fff;
    font-weight: 600;
  }
  .day:disabled {
    color: var(--label-tertiary);
    cursor: default;
  }
  .day:active:not(:disabled):not(.picked) {
    background: var(--fill-secondary);
  }
  .day:focus-visible {
    outline-offset: -2px;
  }
  /* Inside the tint fill of the picked day the ring is --label (black or white), not system blue: see ListRow. */
  .day.picked:focus-visible {
    outline-color: var(--label);
  }
  .time-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 8px;
    padding: 8px 8px 0;
    border-top: var(--hairline) solid var(--separator);
  }
  .time-label {
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
  }
</style>
