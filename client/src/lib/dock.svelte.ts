import { z } from 'zod'
import { uniqueBy } from '@vos/ui'
import type { AppId } from './apps.svelte'

/**
 * What the Dock holds: the apps a person pinned there, in the order they added them, and (worked out from the open
 * windows, never stored) the recent apps that are open but not pinned.
 *
 * The pins belong to this browser, like the settings (`settings.svelte.ts`), and are kept the same way: one versioned
 * key, validated when it is read, followed across the pages of this browser.
 *
 * The list is of ids, and it is the person's: an id that names no app the shell can open right now (not installed
 * yet, because the host has not answered; disabled; uninstalled) stays in it, in its place. Whoever draws the Dock
 * skips it (`shown`) and it is back, where it was, when the app is. Nothing here ever drops an id because the app is
 * missing: only `unpin` removes one.
 */

/** Someone who never chose has the three built-in pages. */
export const DEFAULT_PINS: readonly AppId[] = ['discover', 'files', 'settings']

/**
 * The one key the pins are stored under. The number is the version of the shape below: a change that an older shell
 * could not read gets a new key, and what is under the old one is ignored.
 */
export const STORAGE_KEY = 'vos.dock.v1'

/** How many pins are kept: far more than a Dock shows, few enough that a damaged value cannot grow without end. */
const MAX_PINS = 64
/** How many open, unpinned apps the Dock shows after its divider, as iPadOS does. */
export const MAX_RECENTS = 3

/** What is stored: `{ "pinned": ["discover", "files", …] }`. */
const storedSchema = z.object({
  pinned: z.array(z.unknown())
})

/** Each id once, the first place it has. */
const unique = uniqueBy<AppId>((id) => id)

const isId = (value: unknown): value is AppId =>
  typeof value === 'string' && value.length > 0 && value.length <= 128

/**
 * The pins a stored text holds: the defaults for text that is absent, not JSON, or not the shape above. Within a
 * list, an entry that is not an id is skipped and a repeated id counts once, so one bad entry does not lose the rest.
 * An empty list is a choice (every app unpinned), not a damaged value.
 */
export function parseStored(raw: string | null): AppId[] {
  if (raw === null) return [...DEFAULT_PINS]
  try {
    const parsed = storedSchema.safeParse(JSON.parse(raw))
    if (!parsed.success) return [...DEFAULT_PINS]
    return unique(parsed.data.pinned.filter(isId)).slice(0, MAX_PINS)
  } catch {
    return [...DEFAULT_PINS]
  }
}

/** Where the pins are kept between page loads: the browser's storage, or a test's. */
export interface DockStorage {
  read(): string | null
  write(value: string): void
  /** `listener` runs when another page of this browser stores pins (not for this page's own writes). */
  onChange(listener: (value: string | null) => void): void
}

/**
 * `localStorage`, under `STORAGE_KEY`. A browser without it (blocked site data, some private windows, a bun test)
 * gets `null`, and one that refuses a read or a write loses only that: the pins then last as long as the page.
 */
function browserStorage(): DockStorage | null {
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

export class DockPins {
  #pinned = $state<AppId[]>([...DEFAULT_PINS])
  #storage: DockStorage | null

  constructor(storage: DockStorage | null = browserStorage()) {
    this.#storage = storage
    if (!storage) return
    this.#pinned = parseStored(storage.read())
    storage.onChange((value) => (this.#pinned = parseStored(value)))
  }

  /** Every pinned id, in the person's order, whether or not its app can be opened now. */
  get pinned(): readonly AppId[] {
    return this.#pinned
  }

  isPinned(id: AppId): boolean {
    return this.#pinned.includes(id)
  }

  /** The pinned ids that `exists` says name an app now, in order: what the Dock draws. */
  shown(exists: (id: AppId) => boolean): AppId[] {
    return this.#pinned.filter(exists)
  }

  /** Add to the end of the Dock. An id that is already there keeps its place. */
  pin(id: AppId): void {
    if (!isId(id) || this.isPinned(id) || this.#pinned.length >= MAX_PINS) return
    this.#pinned = [...this.#pinned, id]
    this.#save()
  }

  unpin(id: AppId): void {
    if (!this.isPinned(id)) return
    this.#pinned = this.#pinned.filter((p) => p !== id)
    this.#save()
  }

  /** The whole list is written, absent apps and all: what is stored is what the person chose, never what happens to be installed. */
  #save(): void {
    this.#storage?.write(JSON.stringify({ pinned: this.#pinned }))
  }
}

/**
 * The open apps that are not pinned, most recently used first, at most `limit`: what the Dock shows after its divider,
 * so the app in front can always be reached from the Dock. `open` is every open window's app and how recently it was
 * in front (a bigger `z` is more recent).
 */
export function recentApps(
  open: readonly { appId: AppId; z: number }[],
  isPinned: (id: AppId) => boolean,
  limit: number = MAX_RECENTS
): AppId[] {
  const byRecency = [...open].sort((a, b) => b.z - a.z).map((w) => w.appId)
  return unique(byRecency)
    .filter((id) => !isPinned(id))
    .slice(0, limit)
}

export const dockPins = new DockPins()
