/**
 * Window-scoped keyboard shortcuts (the common `shortcut` prop, spec 1.1): `"cmd+n"`, `"escape"`, `"7"`,
 * `"shift+/"`. Active while focus is inside the window. `cmd` is the platform command key (Meta on Apple
 * platforms, Ctrl elsewhere). A shortcut without a command/ctrl/alt modifier never fires while the user is typing
 * into an editable control.
 */
export interface Combo {
  key: string
  cmd: boolean
  ctrl: boolean
  alt: boolean
  shift: boolean
}

export interface KeyEventLike {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}

const ALIASES: Record<string, string> = {
  esc: 'escape',
  return: 'enter',
  del: 'delete',
  up: 'arrowup',
  down: 'arrowdown',
  left: 'arrowleft',
  right: 'arrowright',
  space: ' ',
  opt: 'alt',
  option: 'alt',
  command: 'cmd',
  control: 'ctrl'
}

export function parseShortcut(spec: string): Combo | null {
  const parts = spec
    .toLowerCase()
    .split('+')
    .map((p) => p.trim())
    .filter(Boolean)
  const key = parts.pop()
  if (!key) return null
  const combo: Combo = {
    key: ALIASES[key] ?? key,
    cmd: false,
    ctrl: false,
    alt: false,
    shift: false
  }
  for (const raw of parts) {
    const mod = ALIASES[raw] ?? raw
    if (mod === 'cmd' || mod === 'ctrl' || mod === 'alt' || mod === 'shift') combo[mod] = true
    else return null
  }
  return combo
}

export function matches(combo: Combo, e: KeyEventLike, apple: boolean): boolean {
  const cmd = apple ? e.metaKey : e.ctrlKey
  const ctrl = apple ? e.ctrlKey : false
  return (
    e.key.toLowerCase() === combo.key &&
    cmd === combo.cmd &&
    ctrl === combo.ctrl &&
    e.altKey === combo.alt &&
    // A printable key like "?" already implies shift: only constrain shift when the combo names it.
    (combo.shift ? e.shiftKey : combo.key.length > 1 ? !e.shiftKey : true)
  )
}

/** True when the shortcut is a bare key that must not steal typing from an editable control. */
export const isBare = (combo: Combo): boolean => !combo.cmd && !combo.ctrl && !combo.alt

export interface Shortcut {
  spec: string
  combo: Combo
  run: () => void
  enabled: () => boolean
}

/** The registry a window keeps: nodes register their `shortcut` while mounted. */
export class ShortcutRegistry {
  private items = new Set<Shortcut>()

  /** Returns the unregister function. An unparsable spec registers nothing. */
  register(spec: string, run: () => void, enabled: () => boolean = () => true): () => void {
    const combo = parseShortcut(spec)
    if (!combo) return () => undefined
    const item: Shortcut = { spec, combo, run, enabled }
    this.items.add(item)
    return () => void this.items.delete(item)
  }

  /** The specs currently registered (the window's command list). */
  list(): string[] {
    return [...this.items].map((i) => i.spec)
  }

  /** Run the first matching, enabled shortcut. Returns true when one ran (the caller prevents default). */
  dispatch(e: KeyEventLike, opts: { apple: boolean; editable: boolean }): boolean {
    for (const item of this.items) {
      if (opts.editable && isBare(item.combo)) continue
      if (!matches(item.combo, e, opts.apple) || !item.enabled()) continue
      item.run()
      return true
    }
    return false
  }
}

/** `aria-keyshortcuts` for a spec: `cmd+n` -> `Meta+N`. */
export function ariaShortcut(spec: string): string | undefined {
  const c = parseShortcut(spec)
  if (!c) return undefined
  const key = c.key.length === 1 ? c.key.toUpperCase() : c.key[0].toUpperCase() + c.key.slice(1)
  return [c.cmd && 'Meta', c.ctrl && 'Control', c.alt && 'Alt', c.shift && 'Shift', key]
    .filter(Boolean)
    .join('+')
}
