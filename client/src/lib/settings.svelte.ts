export type Theme = 'auto' | 'light' | 'dark'

export const accents = [
  { name: 'Blue', value: 'var(--ios-blue)' },
  { name: 'Purple', value: 'var(--ios-purple)' },
  { name: 'Pink', value: 'var(--ios-pink)' },
  { name: 'Red', value: 'var(--ios-red)' },
  { name: 'Orange', value: 'var(--ios-orange)' },
  { name: 'Green', value: 'var(--ios-green)' },
  { name: 'Teal', value: 'var(--ios-teal)' },
  { name: 'Graphite', value: 'var(--ios-gray)' }
] as const

/** System-wide appearance settings, applied to the document root. */
class Settings {
  theme = $state<Theme>('auto')
  /** What the system prefers, followed while `theme` is `auto`. */
  #systemDark = $state(false)
  accent = $state<string>(accents[0].value)
  reduceTransparency = $state(false)
  textScale = $state(100)

  constructor() {
    if (typeof matchMedia !== 'function') return
    const query = matchMedia('(prefers-color-scheme: dark)')
    this.#systemDark = query.matches
    query.addEventListener('change', (e) => (this.#systemDark = e.matches))
  }

  /** The appearance the shell shows now, `auto` resolved: what an app's frame is told. */
  get appearance(): 'light' | 'dark' {
    return this.theme === 'auto' ? (this.#systemDark ? 'dark' : 'light') : this.theme
  }

  apply(root: HTMLElement): void {
    if (this.theme === 'auto') delete root.dataset.theme
    else root.dataset.theme = this.theme
    root.style.setProperty('--tint', this.accent)
    root.style.fontSize = `${this.textScale}%`
    root.classList.toggle('reduce-transparency', this.reduceTransparency)
  }
}

export const settings = new Settings()
