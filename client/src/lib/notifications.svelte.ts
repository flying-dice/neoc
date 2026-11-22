import type { IconName } from '@vos/ui'
import { appNotificationSchema } from '../shell/results'
import { cleanBody, cleanTitle, spokenBody } from '../shell/notificationText'
import { appOf, type AppDefinition, type AppId } from './apps.svelte'
import { windowManager, type WindowManager } from './windowManager.svelte'

/** At most this many alerts are on screen: a fourth takes the place of the oldest. */
export const MAX_BANNERS = 3
/** How long an alert stays when nobody is reading it. The pointer or focus on it holds it; leaving starts the time again. */
export const BANNER_MS = 10_000

/** One alert on screen: what the shell draws of an app's notification. */
export interface Banner {
  id: number
  /** The sender, as the host named it. */
  app: AppId
  /** From the host's list of apps, never from what the app wrote. */
  appName: string
  icon: IconName
  background: string
  /** One line, never empty (an app's notification with no visible title shows the app's name). */
  title: string
  body?: string
  /** What a screen reader is told of the body: the start of it (the screen clamps it too). */
  spokenBody?: string
  /** What the live region says: the sender, the title and the start of the body. */
  announcement: string
}

/** What the alerts need from the shell: the window manager's focus, the app list, and a clock. */
export interface NotificationDeps {
  wm: Pick<WindowManager, 'focused' | 'activeStageId' | 'slideOverHidden' | 'open'>
  appOf(id: AppId): AppDefinition | undefined
  /** Run `run` once, `ms` from now; the result cancels it. */
  schedule(run: () => void, ms: number): () => void
  /** Is the page on screen (its tab or window not hidden)? */
  pageVisible(): boolean
  /**
   * Is the page what the person is using: on screen, and with the keyboard focus (in the page or a frame of it)? A tab
   * in the background, or a browser with another application in front, is not.
   */
  pageInFront(): boolean
  /** `listener` runs when the page is shown or hidden; the result stops it. */
  onPageVisibility(listener: () => void): () => void
}

const browserDeps: NotificationDeps = {
  wm: windowManager,
  appOf,
  schedule: (run, ms) => {
    const timer = setTimeout(run, ms)
    return () => clearTimeout(timer)
  },
  // a bun test has no document: the page is "in front" there, and nothing ever hides it
  pageVisible: () => typeof document === 'undefined' || document.visibilityState === 'visible',
  pageInFront: () =>
    typeof document === 'undefined' ||
    (document.visibilityState === 'visible' && document.hasFocus()),
  onPageVisibility: (listener) => {
    if (typeof document === 'undefined') return () => {}
    document.addEventListener('visibilitychange', listener)
    return () => document.removeEventListener('visibilitychange', listener)
  }
}

/**
 * The system alerts of this client: what `notification.deliver` brings, as the shell shows it (the vos form of
 * UserNotifications' presentation step: the host delivers, this client decides to present).
 *
 *  - The sender's name and icon come from the shell's own app list. A sender that is not a running app here (not
 *    installed, disabled, or not in the list yet) shows nothing, and is logged.
 *  - Not shown on a client where the sender is in front: one of its windows is the focused one, in view, and the page is
 *    what the person is using (a tab in the background does not count: iPadOS presents an app's notification unless
 *    that app is in front on a device in use). Shown on every other client, with or without a window open.
 *  - At most three are on screen. A fourth takes the place of the sender's own oldest alert, so an app that sends a
 *    burst competes with itself first; only a sender with no alert on screen displaces the oldest of the others. An
 *    alert someone is reading (pointer in use on it, or the keyboard on it) is the last to go.
 *  - The ten seconds count only while the page is on screen: an alert that arrives in a hidden tab is still there when
 *    the person comes back, and starts its time then.
 *  - Nothing is kept: an alert that expires, is dismissed or is pushed out is gone, and one that arrives while this
 *    client is not connected never comes (the host does not queue it).
 */
export class Notifications {
  /** On screen, newest first. */
  banners = $state<Banner[]>([])
  #next = 1
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- timers, not state
  #timers = new Map<number, () => void>()
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- ids being read, not state
  #held = new Set<number>()

  constructor(private readonly deps: NotificationDeps = browserDeps) {
    deps.onPageVisibility(() => this.#pageVisibilityChanged())
  }

  /** Take what the host delivered. A message that is not shaped as `notification.deliver` is dropped. */
  deliver(params: unknown): void {
    const parsed = appNotificationSchema.safeParse(params)
    if (!parsed.success) {
      console.warn('[vos] ignored a malformed notification:', parsed.error.issues[0]?.message)
      return
    }
    const { app, title, body } = parsed.data
    const sender = this.deps.appOf(app)
    if (!sender) {
      // an app installed or enabled a moment ago, before this client's list caught up, looks like this
      console.warn(
        `[vos] a notification from "${app}" was not shown: it is not a running app in this shell's list`
      )
      return
    }
    if (this.#senderIsInFront(app)) return
    const text = body === undefined ? '' : cleanBody(body)
    const spoken = text ? spokenBody(text) : ''
    const shownTitle = cleanTitle(title) || sender.name
    const banner: Banner = {
      id: this.#next++,
      app,
      appName: sender.name,
      icon: sender.icon,
      background: sender.background,
      title: shownTitle,
      announcement: `${sender.name}: ${shownTitle}${spoken ? `. ${spoken}` : ''}`
    }
    if (text) {
      banner.body = text
      banner.spokenBody = spoken
    }
    if (this.banners.length >= MAX_BANNERS) {
      const victim = this.#victim(app)
      this.#forget(victim.id)
      this.banners = this.banners.filter((b) => b.id !== victim.id)
    }
    this.banners = [banner, ...this.banners]
    this.#start(banner.id)
  }

  /** Close an alert (the person dismissed it, it expired, or it was acted on). */
  dismiss(id: number): void {
    this.#forget(id)
    this.banners = this.banners.filter((b) => b.id !== id)
  }

  /** The person chose the alert: open the sender's main window (or bring it forward) and close the alert. */
  select(id: number): void {
    const banner = this.banners.find((b) => b.id === id)
    if (!banner) return
    this.dismiss(id)
    this.deps.wm.open(banner.app, { kind: 'main' }) // ignores an app that stopped running meanwhile
  }

  /** The pointer is in use on the alert, or the keyboard is on it: it stays while it is read. */
  hold(id: number): void {
    this.#held.add(id)
    this.#cancel(id)
  }

  /** Nobody is reading it any more: it goes after the full time again. */
  release(id: number): void {
    this.#held.delete(id)
    if (this.banners.some((b) => b.id === id)) this.#start(id)
  }

  /** The apps that stop running take their alerts with them (there is nothing left to open). */
  forgetApps(ids: AppId[]): void {
    for (const banner of this.banners) if (ids.includes(banner.app)) this.dismiss(banner.id)
  }

  /** Is the sender in front here: one of its windows is the focused one, in view, and the page is in use? */
  #senderIsInFront(app: AppId): boolean {
    const { wm } = this.deps
    const win = wm.focused
    if (win?.appId !== app) return false
    const inView =
      win.layout === 'slideover' ? !wm.slideOverHidden : win.stageId === wm.activeStageId
    return inView && this.deps.pageInFront()
  }

  /** The alert a new one from `app` replaces when three are on screen: see the class comment. */
  #victim(app: AppId): Banner {
    const oldestFirst = [...this.banners].reverse()
    const free = oldestFirst.filter((b) => !this.#held.has(b.id))
    return (
      free.find((b) => b.app === app) ??
      free[0] ??
      oldestFirst.find((b) => b.app === app) ??
      oldestFirst[0]!
    )
  }

  /** Hidden: no time is counted. Shown: every alert nobody is reading gets its full time. */
  #pageVisibilityChanged(): void {
    if (!this.deps.pageVisible()) {
      for (const id of [...this.#timers.keys()]) this.#cancel(id)
      return
    }
    for (const banner of this.banners) if (!this.#held.has(banner.id)) this.#start(banner.id)
  }

  #start(id: number): void {
    this.#cancel(id)
    if (!this.deps.pageVisible()) return // its time starts when the page is shown
    this.#timers.set(
      id,
      this.deps.schedule(() => this.dismiss(id), BANNER_MS)
    )
  }

  #cancel(id: number): void {
    this.#timers.get(id)?.()
    this.#timers.delete(id)
  }

  #forget(id: number): void {
    this.#cancel(id)
    this.#held.delete(id)
  }
}

export const notifications = new Notifications()
