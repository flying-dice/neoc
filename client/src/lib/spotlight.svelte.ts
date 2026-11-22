import { uniqueBy, type IconName } from '@vos/ui'
import type { AppId } from './apps.svelte'
import { settingsPanes, type SettingsPane } from './settingsPanes.svelte'
import { matchRank } from './spotlightMatch'

/**
 * System search (Spotlight): what it finds and the state of its panel.
 *
 * What is searched is a list of sources. A source answers a query with results; the results of every source are
 * ranked together (`search`). The sources the shell ships with are made in one place, `builtinSources`: apps,
 * Settings panes and actions. Another source (the text of a note, a to-do: an app's own content) is one more entry in
 * the list given to `new Spotlight`.
 */

/** One thing search found. */
export interface SpotlightResult {
  /** Unique across every source (a source prefixes its own: `app-calculator`). */
  id: string
  title: string
  /** What kind of thing it is, as the row says it: "App", "Settings", "Action". */
  kind: string
  icon: IconName
  /** The plate behind the icon (any CSS background). */
  background?: string
  /**
   * How well it answers the query: lower is better, and results of every source are ordered by it. A name that
   * matches is `MATCH.prefix`..`MATCH.substring` (0..2); anything weaker than a name (a description) starts at
   * `WEAKER`.
   */
  rank: number
  /** Do what the result is for. */
  run(): void
  /** The app whose window is in front once it ran: where the keyboard goes. */
  opens?: AppId
}

/** Something search looks in. */
export interface SpotlightSource {
  id: string
  /**
   * The results for `query`, best first. An empty query (nothing typed yet) asks for suggestions: most sources have
   * none and answer `[]`.
   */
  results(query: string): SpotlightResult[]
}

/** Added to the rank of a match that is not on the name: every name match comes before it. */
export const WEAKER = 3
/** How many results the panel lists. */
export const MAX_RESULTS = 8
/** How many apps are suggested before anything is typed. */
export const MAX_SUGGESTIONS = 4

/**
 * Every source's results for `query`, best first: by rank, then in the order of the sources, then in the order a
 * source gave them. A result whose id was already given is dropped.
 */
export function search(
  sources: readonly SpotlightSource[],
  query: string,
  limit: number = MAX_RESULTS
): SpotlightResult[] {
  const asked = query.trim()
  return uniqueBy<SpotlightResult>((result) => result.id)(
    sources.flatMap((source) => source.results(asked))
  )
    .map((result, order) => ({ result, order }))
    .sort((a, b) => a.result.rank - b.result.rank || a.order - b.order)
    .slice(0, limit)
    .map(({ result }) => result)
}

/** An app as search sees it. */
export interface SearchableApp {
  id: AppId
  name: string
  /** An installed app says what it is for; a built-in page has only its name. */
  description?: string
  icon: IconName
  background?: string
}

/** What the built-in sources need of the shell. */
export interface SpotlightShell {
  /** Every app that can be opened, in the Home Screen's order. */
  apps(): SearchableApp[]
  /** The ids of the apps opened in this session, most recent first. */
  recent(): AppId[]
  openApp(id: AppId): void
  openSettings(pane: SettingsPane): void
  /** Whether the Home Screen is already what is on show. */
  atHome(): boolean
  showHome(): void
}

/**
 * Apps and built-in pages, by name, and installed apps by what they say they are for (below every name match).
 * With nothing typed: the apps opened most recently (the first apps of the Home Screen until any was).
 */
function appsSource(shell: SpotlightShell): SpotlightSource {
  const result = (app: SearchableApp, rank: number): SpotlightResult => ({
    id: `app-${app.id}`,
    title: app.name,
    kind: 'App',
    icon: app.icon,
    background: app.background,
    rank,
    opens: app.id,
    run: () => shell.openApp(app.id)
  })
  return {
    id: 'apps',
    results(query) {
      const apps = shell.apps()
      if (!query) {
        const recent = shell
          .recent()
          .map((id) => apps.find((a) => a.id === id))
          .filter((a): a is SearchableApp => a !== undefined)
        const rest = apps.filter((a) => !recent.includes(a))
        return [...recent, ...rest].slice(0, MAX_SUGGESTIONS).map((app, i) => result(app, i))
      }
      return apps.flatMap((app) => {
        const byName = matchRank(query, app.name)
        if (byName !== null) return [result(app, byName)]
        const byDescription = app.description ? matchRank(query, app.description) : null
        return byDescription === null ? [] : [result(app, WEAKER + byDescription)]
      })
    }
  }
}

/** The panes of Settings, by name: opening one opens Settings on it. */
function settingsSource(shell: SpotlightShell): SpotlightSource {
  return {
    id: 'settings',
    results: (query) =>
      settingsPanes.flatMap((pane) => {
        const rank = matchRank(query, pane.title)
        return rank === null
          ? []
          : [
              {
                id: `settings-${pane.id}`,
                title: pane.title,
                kind: 'Settings',
                icon: pane.icon,
                background: pane.color,
                rank,
                opens: 'settings',
                run: () => shell.openSettings(pane.id)
              }
            ]
      })
  }
}

/** Things the shell can do. "Show Home Screen" is offered while something covers it. */
function actionsSource(shell: SpotlightShell): SpotlightSource {
  return {
    id: 'actions',
    results(query) {
      const rank = matchRank(query, 'Show Home Screen')
      if (rank === null || shell.atHome()) return []
      return [
        {
          id: 'action-home',
          title: 'Show Home Screen',
          kind: 'Action',
          icon: 'house',
          background: 'var(--ios-gray)',
          rank,
          run: () => shell.showHome()
        }
      ]
    }
  }
}

/** The sources the shell ships with, in the order their results are listed when they match equally well. */
export function builtinSources(shell: SpotlightShell): SpotlightSource[] {
  return [appsSource(shell), settingsSource(shell), actionsSource(shell)]
}

/**
 * The apps opened in this session, most recent first: what search suggests before anything is typed. Kept for the
 * page only (a reload starts again from the app the desktop lands on).
 */
export class RecentlyOpened {
  #ids = $state<AppId[]>([])

  get ids(): readonly AppId[] {
    return this.#ids
  }

  /** `id` was just opened or brought to the front. */
  note(id: AppId): void {
    if (this.#ids[0] === id) return
    this.#ids = [id, ...this.#ids.filter((other) => other !== id)].slice(0, 16)
  }
}

/** The state of the search panel: whether it is open, what was typed, what was found and which result is highlighted. */
export class Spotlight {
  #sources: readonly SpotlightSource[]
  #open = $state(false)
  #query = $state('')
  /** The result the person highlighted; null until they do, and again when the query changes (the top one then). */
  #chosen = $state<string | null>(null)
  #results = $derived.by(() => (this.#open ? search(this.#sources, this.#query) : []))

  constructor(sources: readonly SpotlightSource[]) {
    this.#sources = sources
  }

  get open(): boolean {
    return this.#open
  }

  get query(): string {
    return this.#query
  }
  /** Typing filters as it goes, and the top result is the one Enter opens again. */
  set query(value: string) {
    if (value === this.#query) return
    this.#query = value
    this.#chosen = null
  }

  /** Nothing is typed: the results are suggestions. */
  get suggesting(): boolean {
    return this.#query.trim() === ''
  }

  get results(): SpotlightResult[] {
    return this.#results
  }

  /** The highlighted result: the one the person moved to if it is still listed, else the top one. */
  get activeId(): string | null {
    const results = this.#results
    return results.some((r) => r.id === this.#chosen) ? this.#chosen : (results[0]?.id ?? null)
  }

  /** What a screen reader is told about the list. */
  get status(): string {
    if (!this.#open) return ''
    const count = this.#results.length
    if (this.suggesting)
      return count === 0 ? '' : count === 1 ? '1 suggestion' : `${count} suggestions`
    return count === 0 ? 'No results' : count === 1 ? '1 result' : `${count} results`
  }

  /** Open the panel, empty. */
  show(): void {
    if (this.#open) return
    this.#query = ''
    this.#chosen = null
    this.#open = true
  }

  /** Close it without opening anything. */
  close(): void {
    this.#open = false
    this.#query = ''
    this.#chosen = null
  }

  toggle(): void {
    if (this.#open) this.close()
    else this.show()
  }

  /** Highlight the next (1) or previous (-1) result; the ends do not wrap. */
  move(delta: 1 | -1): void {
    const results = this.#results
    if (results.length === 0) return
    const at = results.findIndex((r) => r.id === this.activeId)
    this.#chosen = results[Math.min(Math.max(at + delta, 0), results.length - 1)].id
  }

  highlight(id: string): void {
    if (this.#results.some((r) => r.id === id)) this.#chosen = id
  }

  /**
   * Open a result (the highlighted one by default): it runs, and the panel closes. Returns the result, so the
   * caller can send the keyboard after it, or null when there was nothing to open (the panel stays as it is).
   */
  activate(id: string | null = this.activeId): SpotlightResult | null {
    const result = this.#results.find((r) => r.id === id)
    if (!result) return null
    this.close()
    result.run()
    return result
  }
}
