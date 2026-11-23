import { describe, expect, test } from 'bun:test'
import {
  accentColor,
  accents,
  DEFAULTS,
  parseStored,
  Settings,
  STORAGE_KEY,
  type SettingsStorage,
  type SettingsValues
} from './settings.svelte'

/** A storage the test can read, fill and change from "another page". */
function memory(initial: string | null = null): SettingsStorage & {
  value: string | null
  writes: number
  /** Another page of this browser stored `value`. */
  changeElsewhere(value: string | null): void
} {
  const listeners: ((value: string | null) => void)[] = []
  const store = {
    value: initial,
    writes: 0,
    read: () => store.value,
    write: (value: string) => {
      store.value = value
      store.writes++
    },
    onChange: (listener: (value: string | null) => void) => void listeners.push(listener),
    changeElsewhere: (value: string | null) => {
      store.value = value
      listeners.forEach((l) => l(value))
    }
  }
  return store
}

const valuesOf = (s: Settings): SettingsValues => ({
  theme: s.theme,
  accent: s.accent,
  textScale: s.textScale,
  reduceTransparency: s.reduceTransparency,
  banners: s.banners,
  statusBar: s.statusBar
})

const chosen: SettingsValues = {
  theme: 'dark',
  accent: 'green',
  textScale: 115,
  reduceTransparency: true,
  banners: false,
  statusBar: true
}

/** What `apply` needs of an element. */
function root(): HTMLElement & { props: Record<string, string>; classes: Set<string> } {
  const props: Record<string, string> = {}
  const classes = new Set<string>()
  return {
    props,
    classes,
    dataset: {},
    style: { setProperty: (name: string, value: string) => void (props[name] = value) },
    classList: {
      toggle: (name: string, on: boolean) => (on ? classes.add(name) : classes.delete(name))
    }
  } as unknown as HTMLElement & { props: Record<string, string>; classes: Set<string> }
}

describe('with nothing stored', () => {
  test('the defaults: automatic theme, blue, normal text, materials, banners on, no status bar', () => {
    const settings = new Settings(memory())
    expect(valuesOf(settings)).toEqual({
      theme: 'auto',
      accent: 'blue',
      textScale: 100,
      reduceTransparency: false,
      banners: true,
      statusBar: false
    })
  })

  test('nothing is written until the person changes something', () => {
    const storage = memory()
    new Settings(storage)
    expect(storage.writes).toBe(0)
    expect(storage.value).toBeNull()
  })

  test('the key says which version of the shape is under it', () => {
    expect(STORAGE_KEY).toBe('vos.settings.v1')
  })
})

describe('a change', () => {
  test('is stored as it is made, with everything else as it stands', () => {
    const storage = memory()
    const settings = new Settings(storage)
    settings.theme = 'dark'
    expect(JSON.parse(storage.value!)).toEqual({ ...DEFAULTS, theme: 'dark' })
    settings.accent = 'green'
    settings.textScale = 115
    settings.reduceTransparency = true
    settings.banners = false
    settings.statusBar = true
    expect(JSON.parse(storage.value!)).toEqual(chosen)
    expect(storage.writes).toBe(6)
  })

  test('survives a reload: the next page starts with what was chosen', () => {
    const storage = memory()
    const before = new Settings(storage)
    before.theme = 'dark'
    before.accent = 'green'
    before.textScale = 115
    before.reduceTransparency = true
    before.banners = false
    before.statusBar = true
    expect(valuesOf(new Settings(storage))).toEqual(chosen)
  })

  test('reaches another page of the same browser that is open, which does not write it back', () => {
    const storage = memory()
    const other = new Settings(storage)
    storage.changeElsewhere(JSON.stringify(chosen))
    expect(valuesOf(other)).toEqual(chosen)
    expect(storage.writes).toBe(0)
  })

  test('another page clearing the storage puts an open page back on the defaults', () => {
    const storage = memory(JSON.stringify(chosen))
    const settings = new Settings(storage)
    storage.changeElsewhere(null)
    expect(valuesOf(settings)).toEqual({ ...DEFAULTS })
  })
})

describe('what is read is checked', () => {
  test('text that is not JSON, or not an object, gives the defaults', () => {
    for (const raw of ['', 'dark', '{', 'null', '42', '"dark"', '[]', '[{"theme":"dark"}]'])
      expect(parseStored(raw), raw).toEqual({ ...DEFAULTS })
  })

  test('a value that is not one a person could have chosen falls back alone; the rest is kept', () => {
    const bad: [keyof SettingsValues, unknown[]][] = [
      ['theme', ['sepia', '', 1, null, true]],
      ['accent', ['Green', 'var(--ios-green)', 'url(https://example.test/x)', '#00ff00', 7, null]],
      ['textScale', [80, 130, 101, 112.5, '115', null, Number.NaN]],
      ['reduceTransparency', ['true', 1, null]],
      ['banners', ['false', 0, null]],
      ['statusBar', ['true', 1, null]]
    ]
    for (const [field, values] of bad) {
      for (const value of values) {
        const raw = JSON.stringify({ ...chosen, [field]: value })
        expect(parseStored(raw), raw).toEqual({ ...chosen, [field]: DEFAULTS[field] })
      }
    }
  })

  test('an older value with a setting missing takes the default for it, and one with extras ignores them', () => {
    expect(parseStored(JSON.stringify({ theme: 'dark', accent: 'green' }))).toEqual({
      ...DEFAULTS,
      theme: 'dark',
      accent: 'green'
    })
    expect(parseStored(JSON.stringify({ ...chosen, agents: true, wallpaper: 'x' }))).toEqual(chosen)
  })

  test('every text size the slider offers is accepted', () => {
    for (let size = 85; size <= 125; size += 5)
      expect(parseStored(JSON.stringify({ textScale: size })).textScale).toBe(size)
  })

  test('a page that starts on a bad stored value shows the defaults and leaves the storage alone', () => {
    const storage = memory('{"theme":"sepia"')
    expect(valuesOf(new Settings(storage))).toEqual({ ...DEFAULTS })
    expect(storage.writes).toBe(0)
  })
})

describe('a browser without storage', () => {
  test('starts on the defaults and takes changes, which last as long as the page', () => {
    const settings = new Settings(null)
    settings.theme = 'light'
    settings.banners = false
    expect(valuesOf(settings)).toEqual({ ...DEFAULTS, theme: 'light', banners: false })
    expect(valuesOf(new Settings(null))).toEqual({ ...DEFAULTS })
  })

  test('is what a bun test has: the shared store is on the defaults', async () => {
    const { settings } = await import('./settings.svelte')
    expect(settings.theme).toBe('auto')
  })
})

describe('a forced theme (?theme=)', () => {
  test('shows on this page over what is stored, and the stored choice stays', () => {
    const storage = memory(JSON.stringify(chosen))
    const settings = new Settings(storage)
    settings.forceTheme('light')
    expect(settings.theme).toBe('light')
    expect(settings.appearance).toBe('light')
    expect(JSON.parse(storage.value!)).toEqual(chosen)
  })

  test('the page stores nothing afterwards, and does not follow other pages', () => {
    const storage = memory()
    const settings = new Settings(storage)
    settings.forceTheme('dark')
    settings.accent = 'green'
    expect(storage.writes).toBe(0)
    storage.changeElsewhere(JSON.stringify({ ...DEFAULTS, theme: 'light' }))
    expect(settings.theme).toBe('dark')
    expect(settings.accent).toBe('green')
  })
})

describe('the appearance an app is told', () => {
  test('is the chosen theme; automatic is light where nothing says what the system prefers', () => {
    const settings = new Settings(null)
    expect(settings.appearance).toBe('light')
    settings.theme = 'dark'
    expect(settings.appearance).toBe('dark')
    settings.theme = 'light'
    expect(settings.appearance).toBe('light')
  })

  test('is the stored theme from the first moment (the standalone page of an app reads it once, to load the app)', () => {
    expect(new Settings(memory(JSON.stringify(chosen))).appearance).toBe('dark')
  })
})

describe('applied to the document', () => {
  test('the theme, the tint, the text scale as a factor, and the transparency class', () => {
    const settings = new Settings(memory(JSON.stringify(chosen)))
    const el = root()
    settings.apply(el)
    expect(el.dataset.theme).toBe('dark')
    expect(el.props).toEqual({ '--tint': 'light-dark(#1f7a36, #30d158)', '--text-scale': '1.15' })
    expect([...el.classes]).toEqual(['reduce-transparency'])
  })

  test('automatic leaves the theme to the system, and the defaults undo what was applied', () => {
    const settings = new Settings(null)
    const el = root()
    settings.theme = 'dark'
    settings.reduceTransparency = true
    settings.apply(el)
    settings.theme = 'auto'
    settings.reduceTransparency = false
    settings.apply(el)
    expect('theme' in el.dataset).toBe(false)
    expect(el.props).toEqual({ '--tint': 'light-dark(#0066cc, #409cff)', '--text-scale': '1' })
    expect([...el.classes]).toEqual([])
  })
})

// An accent is drawn both ways: as text and glyphs on a window's surfaces, and as a fill under white text (the selected
// row, a filled button: `--tint-fill` in @vos/ui, the accent itself in light and mixed 70% with black in oklab in dark).
describe('every accent can be read', () => {
  type Rgb = [number, number, number]
  const rgb = (hex: string): Rgb => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as Rgb
  const linear = (c: number): number =>
    c / 255 <= 0.04045 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4
  const luminance = ([r, g, b]: Rgb): number =>
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
  const ratio = (a: Rgb, b: Rgb): number => {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number]
    return (hi + 0.05) / (lo + 0.05)
  }
  /** `color-mix(in oklab, c 70%, black)`. */
  const darkFill = ([r, g, b]: Rgb): Rgb => {
    const [lr, lg, lb] = [linear(r), linear(g), linear(b)]
    const l0 = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
    const m0 = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
    const s0 = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
    const [L, A, B] = [
      0.2104542553 * l0 + 0.793617785 * m0 - 0.0040720468 * s0,
      1.9779984951 * l0 - 2.428592205 * m0 + 0.4505937099 * s0,
      0.0259040371 * l0 + 0.7827717662 * m0 - 0.808675766 * s0
    ].map((v) => v * 0.7) as Rgb
    const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
    const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
    const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
    const gamma = (v: number): number =>
      Math.round(
        255 * Math.min(1, Math.max(0, v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055))
      )
    return [
      gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
      gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
      gamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s)
    ]
  }
  const WHITE: Rgb = [255, 255, 255]
  // the window surfaces of @vos/ui tokens.css: grouped background and the rows on it, light and dark
  const LIGHT_SURFACES = ['#f2f2f7', '#ffffff']
  const DARK_SURFACES = ['#1c1c1e', '#2c2c2e']

  for (const accent of accents) {
    test(`${accent.name}: 4.5:1 as text on a window, and under white text as a fill, light and dark`, () => {
      for (const surface of LIGHT_SURFACES)
        expect(
          ratio(rgb(accent.light), rgb(surface)),
          `light on ${surface}`
        ).toBeGreaterThanOrEqual(4.5)
      for (const surface of DARK_SURFACES)
        expect(ratio(rgb(accent.dark), rgb(surface)), `dark on ${surface}`).toBeGreaterThanOrEqual(
          4.5
        )
      expect(ratio(WHITE, rgb(accent.light)), 'white on the light fill').toBeGreaterThanOrEqual(4.5)
      expect(
        ratio(WHITE, darkFill(rgb(accent.dark))),
        'white on the dark fill'
      ).toBeGreaterThanOrEqual(4.5)
    })
  }

  test("the colour of an accent is its pair, by theme; blue is the tokens' system blue", () => {
    expect(accentColor('green')).toBe('light-dark(#1f7a36, #30d158)')
    expect(accentColor('blue')).toBe('light-dark(#0066cc, #409cff)')
  })
})
