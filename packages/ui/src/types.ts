export interface AlertAction {
  label: string
  style?: 'default' | 'cancel' | 'destructive'
  /** Bold "preferred" action: takes initial focus and runs on Enter from a field. Cancel is bold by default. */
  preferred?: boolean
  /** Stable id for the test id (default: a slug of the label). */
  key?: string
  disabled?: boolean
  /** Receives the prompt field values (`{}` when the alert has none). */
  onclick?: (values: Record<string, string>) => void
}

/** A prompt field of an `Alert`. */
export interface AlertField {
  key: string
  placeholder?: string
  secure?: boolean
}

export interface DockApp {
  id: string
  name: string
  icon: import('./icons').IconName
  background?: string
  badge?: number
  running?: boolean
}

/** One row of a choice list (`OptionList`, a Picker). */
export interface OptionItem {
  /** Stable id: used in the test id and the callbacks. */
  key: string
  label: string
  icon?: import('./icons').IconName
  disabled?: boolean
}

/** A menu row. `submenu` makes it a branch; `checked` draws a checkmark (a toggle or the chosen option). */
export interface MenuEntry {
  label: string
  shortcut?: string
  disabled?: boolean
  checked?: boolean
  icon?: import('./icons').IconName
  /** Red: the item removes or discards something. */
  destructive?: boolean
  /** Stable id for the test id (default: a slug of the label). */
  key?: string
  onclick?: () => void
  submenu?: MenuItem[]
}

export type MenuItem =
  | MenuEntry
  | { separator: true }
  /** A section heading: a small caption above the items that follow it. */
  | { heading: string }

export interface MenuDefinition {
  /** Menu bar title. */
  title: string
  /** Render the title bold (the active app's name). */
  bold?: boolean
  /** Render an icon instead of text (the system menu). */
  icon?: import('./icons').IconName
  items: MenuItem[]
}

export interface Frame {
  x: number
  y: number
  width: number
  height: number
}

/** iPadOS multitasking layouts a window can adopt. */
export type WindowLayout = 'floating' | 'fullscreen' | 'split-left' | 'split-right' | 'slideover'

/** A group of windows shown together on stage (Stage Manager "app group"). */
export interface StageSummary {
  id: string
  /** One entry per app in the group; `id` is the app id (stable, used for test ids). */
  apps: { id: string; name: string; icon: import('./icons').IconName; background?: string }[]
}
