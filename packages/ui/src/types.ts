export interface AlertAction {
  label: string
  style?: 'default' | 'cancel' | 'destructive'
  /** Bold "preferred" action: takes initial focus. Cancel is bold by default. */
  preferred?: boolean
  onclick?: () => void
}

export interface DockApp {
  id: string
  name: string
  icon: import('./icons').IconName
  background?: string
  running?: boolean
}

/** One choice of a context menu. */
export interface MenuItem {
  /** Stable within the menu: what `onselect` is called with, and the item's test id suffix. */
  id: string
  label: string
  icon?: import('./icons').IconName
  /** Red: an action that removes or deletes. */
  destructive?: boolean
}

/** A box in viewport coordinates (as `getBoundingClientRect` gives): what a context menu opens beside. */
export interface AnchorRect {
  left: number
  top: number
  width: number
  height: number
}

/** One row of Spotlight's results. */
export interface SpotlightItem {
  /** Unique among the results shown: the option's id and its test id suffix. */
  id: string
  title: string
  /** What kind of thing it is, shown at the row's end ("App", "Settings"). */
  kind?: string
  icon: import('./icons').IconName
  /** The plate behind the glyph (any CSS background): an app's own, or a system colour. */
  background?: string
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
