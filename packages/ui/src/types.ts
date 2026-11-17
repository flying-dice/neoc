export interface AlertAction {
  label: string
  style?: 'default' | 'cancel' | 'destructive'
  /** Bold "preferred" action. Cancel actions are bold by default. */
  preferred?: boolean
  onclick?: () => void
}

export interface DockApp {
  id: string
  name: string
  icon: import('./icons').IconName
  background?: string
  badge?: number
  running?: boolean
}

export type MenuItem =
  | { label: string; shortcut?: string; disabled?: boolean; checked?: boolean; onclick?: () => void }
  | { separator: true }

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
