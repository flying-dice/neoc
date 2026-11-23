import { z } from 'zod'

export type Theme = 'auto' | 'light' | 'dark'

/**
 * The accent colours Settings offers, each as the pair the shell draws: `light` on a light theme, `dark` on a dark
 * one. They are the "Increase Contrast" variants of the system colours, not the plain ones, because an accent is
 * both ink and paper: text and glyphs in the accent on a window's background, and white text on a fill of it (the
 * selected row, a filled button). Plain system green is 2.2:1 under white text; every pair here holds 4.5:1 both ways
 * in both themes (`settings.test.ts` asserts it). Blue is the tokens' own `--ios-blue`.
 */
export const accents = [
  { id: 'blue', name: 'Blue', light: '#0066cc', dark: '#409cff' },
  { id: 'purple', name: 'Purple', light: '#8944ab', dark: '#da8fff' },
  { id: 'pink', name: 'Pink', light: '#d30f45', dark: '#ff6482' },
  { id: 'red', name: 'Red', light: '#d70015', dark: '#ff6961' },
  { id: 'orange', name: 'Orange', light: '#b25000', dark: '#ff9f0a' },
  { id: 'green', name: 'Green', light: '#1f7a36', dark: '#30d158' },
  { id: 'teal', name: 'Teal', light: '#007a8a', dark: '#40c8e0' },
  { id: 'graphite', name: 'Graphite', light: '#636366', dark: '#aeaeb2' }
] as const
export type Accent = (typeof accents)[number]['id']

/** The CSS colour of an accent: its light or dark value, by the theme of where it is used. */
export const accentColor = (id: Accent): string => {
  const accent = accents.find((a) => a.id === id) ?? accents[0]
  return `light-dark(${accent.light}, ${accent.dark})`
}

/** The text sizes Settings offers, in percent of the default size. */
export const TEXT_SCALE = { min: 85, max: 125, step: 5 } as const

/** What a person can choose in Settings. All of it belongs to this browser: nothing here is the host's. */
export interface SettingsValues {
  theme: Theme
  accent: Accent
  /** Percent of the default text size, within `TEXT_SCALE`. */
  textScale: number
  reduceTransparency: boolean
  /** Show an app's notifications as banners (`Notifications`). */
  banners: boolean
  /** A feature (`Settings > Features`): the desktop's own status bar, with the time, the date and search. */
  statusBar: boolean
}

export const DEFAULTS: Readonly<SettingsValues> = {
  theme: 'auto',
  accent: 'blue',
  textScale: 100,
  reduceTransparency: false,
  banners: true,
  statusBar: false
}

/**
 * The one key the settings are stored under. The number is the version of the shape below: a change that an older
 * shell could not read gets a new key, and what is under the old one is ignored.
 */
export const STORAGE_KEY = 'vos.settings.v1'

/** What is stored. Each field falls back to its default on its own, so one bad or missing value does not lose the rest. */
const storedSchema = z.object({
  theme: z.enum(['auto', 'light', 'dark']).catch(DEFAULTS.theme),
  accent: z.enum(accents.map((a) => a.id)).catch(DEFAULTS.accent),
  textScale: z
    .number()
    .min(TEXT_SCALE.min)
    .max(TEXT_SCALE.max)
    .multipleOf(TEXT_SCALE.step)
    .catch(DEFAULTS.textScale),
  reduceTransparency: z.boolean().catch(DEFAULTS.reduceTransparency),
  banners: z.boolean().catch(DEFAULTS.banners),
  statusBar: z.boolean().catch(DEFAULTS.statusBar)
})

/** The settings a stored text holds: the defaults for text that is absent, not JSON, or not an object. */
export function parseStored(raw: string | null): SettingsValues {
  if (raw === null) return { ...DEFAULTS }
  try {
    const parsed = storedSchema.safeParse(JSON.parse(raw))
    return parsed.success ? parsed.data : { ...DEFAULTS }
  } catch {
    return { ...DEFAULTS }
  }
}

/** Where the settings are kept between page loads: the browser's storage, or a test's. */
export interface SettingsStorage {
  read(): string | null
  write(value: string): void
  /** `listener` runs when another page of this browser stores settings (not for this page's own writes). */
  onChange(listener: (value: string | null) => void): void
}

/**
 * `localStorage`, under `STORAGE_KEY`. A browser without it (blocked site data, some private windows, a bun test)
 * gets `null`, and one that refuses a read or a write loses only that: the settings then last as long as the page.
 */
function browserStorage(): SettingsStorage | null {
  let storage: Storage
  try {
    if (typeof localStorage === 'undefined') return null
    storage = localStorage
  } catch {
    return null // reading the property itself is refused when site data is blocked
  }
  return {
    read: () => {
      try {
        return storage.getItem(STORAGE_KEY)
      } catch {
        return null
      }
    },
    write: (value) => {
      try {
        storage.setItem(STORAGE_KEY, value)
      } catch {
        // full, or refused: the choice still holds for this page
      }
    },
    onChange: (listener) => {
      if (typeof window === 'undefined') return
      window.addEventListener('storage', (e) => {
        // `key` is null when the whole storage was cleared
        if (e.storageArea === storage && (e.key === STORAGE_KEY || e.key === null))
          listener(e.key === null ? null : e.newValue)
      })
    }
  }
}

/**
 * The settings of this browser: how the shell looks (applied to the document root, and told to every app's frame) and
 * whether notifications show. A change is stored as it is made and read back on the next load, here and on the
 * standalone page of an app (`wrapper/main.svelte.ts`); another page of the same browser that is open follows it.
 */
export class Settings {
  #theme = $state<Theme>(DEFAULTS.theme)
  #accent = $state<Accent>(DEFAULTS.accent)
  #textScale = $state(DEFAULTS.textScale)
  #reduceTransparency = $state(DEFAULTS.reduceTransparency)
  #banners = $state(DEFAULTS.banners)
  #statusBar = $state(DEFAULTS.statusBar)
  /** What the system prefers, followed while `theme` is `auto`. */
  #systemDark = $state(false)
  #storage: SettingsStorage | null

  constructor(storage: SettingsStorage | null = browserStorage()) {
    this.#storage = storage
    if (storage) {
      this.#take(parseStored(storage.read()))
      storage.onChange((value) => {
        if (this.#storage) this.#take(parseStored(value))
      })
    }
    if (typeof matchMedia !== 'function') return
    const query = matchMedia('(prefers-color-scheme: dark)')
    this.#systemDark = query.matches
    query.addEventListener('change', (e) => (this.#systemDark = e.matches))
  }

  get theme(): Theme {
    return this.#theme
  }
  set theme(value: Theme) {
    this.#theme = value
    this.#save()
  }

  get accent(): Accent {
    return this.#accent
  }
  set accent(value: Accent) {
    this.#accent = value
    this.#save()
  }

  get textScale(): number {
    return this.#textScale
  }
  set textScale(value: number) {
    this.#textScale = value
    this.#save()
  }

  get reduceTransparency(): boolean {
    return this.#reduceTransparency
  }
  set reduceTransparency(value: boolean) {
    this.#reduceTransparency = value
    this.#save()
  }

  get banners(): boolean {
    return this.#banners
  }
  set banners(value: boolean) {
    this.#banners = value
    this.#save()
  }

  get statusBar(): boolean {
    return this.#statusBar
  }
  set statusBar(value: boolean) {
    this.#statusBar = value
    this.#save()
  }

  /** The appearance the shell shows now, `auto` resolved: what an app's frame is told. */
  get appearance(): 'light' | 'dark' {
    return this.#theme === 'auto' ? (this.#systemDark ? 'dark' : 'light') : this.#theme
  }

  /**
   * Show this theme on this page whatever is stored (`?theme=`, for tests and screenshots). The page is then on its
   * own: it stores nothing and follows no other page, so a forced theme never becomes the person's choice.
   */
  forceTheme(theme: 'light' | 'dark'): void {
    this.#storage = null
    this.#theme = theme
  }

  apply(root: HTMLElement): void {
    if (this.#theme === 'auto') delete root.dataset.theme
    else root.dataset.theme = this.#theme
    root.style.setProperty('--tint', accentColor(this.#accent))
    // the type tokens multiply by this (`@vos/ui` tokens.css)
    root.style.setProperty('--text-scale', String(this.#textScale / 100))
    root.classList.toggle('reduce-transparency', this.#reduceTransparency)
  }

  #take(values: SettingsValues): void {
    this.#theme = values.theme
    this.#accent = values.accent
    this.#textScale = values.textScale
    this.#reduceTransparency = values.reduceTransparency
    this.#banners = values.banners
    this.#statusBar = values.statusBar
  }

  #save(): void {
    const values: SettingsValues = {
      theme: this.#theme,
      accent: this.#accent,
      textScale: this.#textScale,
      reduceTransparency: this.#reduceTransparency,
      banners: this.#banners,
      statusBar: this.#statusBar
    }
    this.#storage?.write(JSON.stringify(values))
  }
}

export const settings = new Settings()
