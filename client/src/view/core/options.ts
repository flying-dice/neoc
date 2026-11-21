/**
 * Picker options and selection (`Picker.options`, `Picker.selection`). Options are a literal list of
 * `{value, label, icon?, disabled?}` or the rows of a source (fields `value` and `label`); the selection is one
 * value, or a list when `multiple`. Pure: adapters call it inside `$derived`.
 */
import type { Json, Row } from '../protocol'

export interface PickerOption {
  /** Stable string form of `value` (the kit's option key and the test id part). */
  key: string
  value: Json
  label: string
  icon?: string
  disabled: boolean
}

/** The string a value is known by: itself for strings, JSON otherwise (`1`, `true`, `{"a":1}`). */
export const keyOf = (value: Json | undefined): string =>
  typeof value === 'string' ? value : JSON.stringify(value ?? null)

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

function optionOf(raw: unknown): PickerOption | null {
  if (!isRecord(raw) || !('value' in raw)) return null
  const value = raw.value as Json
  const label = raw.label === undefined || raw.label === null ? keyOf(value) : String(raw.label)
  return {
    key: keyOf(value),
    value,
    label,
    icon: typeof raw.icon === 'string' && raw.icon ? raw.icon : undefined,
    disabled: raw.disabled === true
  }
}

/** A literal list becomes options; malformed entries and repeated values are dropped (the first wins). */
export function parseOptions(raw: unknown): PickerOption[] {
  if (!Array.isArray(raw)) return []
  const seen = new Set<string>()
  const out: PickerOption[] = []
  for (const entry of raw) {
    const option = optionOf(entry)
    if (!option || seen.has(option.key)) continue
    seen.add(option.key)
    out.push(option)
  }
  return out
}

/** Options from a source's window: row fields `value`, `label`, optional `icon`, `disabled`. */
export function optionsFromRows(rows: readonly Row[]): PickerOption[] {
  return parseOptions(
    rows.map((r) => ({ value: r.value, label: r.label, icon: r.icon, disabled: r.disabled }))
  )
}

/** The selected keys: one for a single selection, any number when `multiple`. */
export function selectedKeys(selection: unknown, multiple: boolean): string[] {
  if (multiple) return Array.isArray(selection) ? selection.map((v) => keyOf(v as Json)) : []
  return selection === undefined || selection === null ? [] : [keyOf(selection as Json)]
}

/** The new selection after choosing `value`: replaces a single selection, toggles in a multiple one. */
export function choose(current: unknown, value: Json, multiple: boolean): Json {
  if (!multiple) return value
  const list = Array.isArray(current) ? (current as Json[]) : []
  const key = keyOf(value)
  return list.some((v) => keyOf(v) === key)
    ? list.filter((v) => keyOf(v) !== key)
    : [...list, value]
}

/** The option for a key, or undefined. */
export const optionFor = (
  options: readonly PickerOption[],
  key: string
): PickerOption | undefined => options.find((o) => o.key === key)

/** The label a closed menu picker shows: the chosen option, several joined, or the placeholder. */
export function selectionLabel(
  options: readonly PickerOption[],
  keys: readonly string[],
  none = 'Select'
): string {
  const labels = keys.map((k) => optionFor(options, k)?.label).filter((l): l is string => !!l)
  if (labels.length === 0) return none
  return labels.length <= 2 ? labels.join(', ') : `${labels.length} selected`
}
