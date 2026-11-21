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
  accent = $state<string>(accents[0].value)
  reduceTransparency = $state(false)
  textScale = $state(100)

  apply(root: HTMLElement): void {
    if (this.theme === 'auto') delete root.dataset.theme
    else root.dataset.theme = this.theme
    root.style.setProperty('--tint', this.accent)
    root.style.fontSize = `${this.textScale}%`
    root.classList.toggle('reduce-transparency', this.reduceTransparency)
  }
}

export const settings = new Settings()
