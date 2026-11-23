import type { IconName } from '@vos/ui'

/**
 * The panes of Settings, by id: what the Settings page lists, and what system search can open it on.
 */
export const settingsPanes = [
  { id: 'general', title: 'General', icon: 'gear', color: 'var(--ios-gray)' },
  { id: 'appearance', title: 'Appearance', icon: 'sun', color: 'var(--ios-blue)' },
  { id: 'features', title: 'Features', icon: 'flag', color: 'var(--ios-orange)' },
  { id: 'devices', title: 'Devices', icon: 'globe', color: 'var(--ios-green)' }
] as const satisfies readonly { id: string; title: string; icon: IconName; color: string }[]

export type SettingsPane = (typeof settingsPanes)[number]['id']

/**
 * A pane the shell asked Settings to show (system search opening "Appearance"). The Settings page takes it when it
 * is drawn, or at once when it is already open; until then it waits here.
 */
class SettingsNavigation {
  #asked = $state<SettingsPane | null>(null)

  /** The pane asked for and not yet shown. */
  get asked(): SettingsPane | null {
    return this.#asked
  }

  show(pane: SettingsPane): void {
    this.#asked = pane
  }

  /** The Settings page has shown it. */
  done(): void {
    this.#asked = null
  }
}

export const settingsNavigation = new SettingsNavigation()
