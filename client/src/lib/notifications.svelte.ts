import type { IconName } from '@vos/ui'
import { appNotificationSchema } from '../shell/results'
import { cleanBody, cleanTitle } from '../shell/notificationText'
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
}

/** What the alerts need from the shell: the window manager's focus, the app list, and a clock. */
export interface NotificationDeps {
  wm: Pick<WindowManager, 'focused' | 'activeStageId' | 'slideOverHidden' | 'open'>
  appOf(id: AppId): AppDefinition | undefined
  /** Run `run` once, `ms` from now; the result cancels it. */
  schedule(run: () => void, ms: number): () => void
}

const browserDeps: NotificationDeps = {
  wm: windowManager,
  appOf,
  schedule: (run, ms) => {
    const timer = setTimeout(run, ms)
    return () => clearTimeout(timer)
  }
}

/**
 * The system alerts of this client: what `notification.deliver` brings, as the shell shows it (the vos form of
 * UserNotifications' presentation step: the host delivers, this client decides to present).
 *
 *  - The sender's name and icon come from the shell's own app list. A sender that is not a running app here (not
 *    installed, disabled, or not in the list yet) shows nothing.
 *  - Not shown on a client where one of the sender's windows has focus (iPadOS does not present an app's notification
 *    while that app is in front); shown on every other client, with or without a window open for the sender.
 *  - Nothing is kept: an alert that expires, is dismissed or is pushed out by newer ones is gone, and one that arrives
 *    while this client is not connected never comes (the host does not queue it).
 */
export class Notifications {
  /** On screen, newest first. */
  banners = $state<Banner[]>([])
  #next = 1
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- timers, not state
  #timers = new Map<number, () => void>()

  constructor(private readonly deps: NotificationDeps = browserDeps) {}

  /** Take what the host delivered. A message that is not shaped as `notification.deliver` is dropped. */
  deliver(params: unknown): void {
    const parsed = appNotificationSchema.safeParse(params)
    if (!parsed.success) {
      console.warn('[vos] ignored a malformed notification:', parsed.error.issues[0]?.message)
      return
    }
    const { app, title, body } = parsed.data
    const sender = this.deps.appOf(app)
    if (!sender || this.#senderHasFocus(app)) return
    const banner: Banner = {
      id: this.#next++,
      app,
      appName: sender.name,
      icon: sender.icon,
      background: sender.background,
      title: cleanTitle(title) || sender.name
    }
    const text = body === undefined ? '' : cleanBody(body)
    if (text) banner.body = text
    this.banners = [banner, ...this.banners]
    for (const old of this.banners.slice(MAX_BANNERS)) this.#forget(old.id)
    this.banners = this.banners.slice(0, MAX_BANNERS)
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

  /** The pointer or focus is on the alert: it stays while it is read. */
  hold(id: number): void {
    this.#timers.get(id)?.()
    this.#timers.delete(id)
  }

  /** The pointer and focus have left: it goes after the full time again. */
  release(id: number): void {
    if (this.banners.some((b) => b.id === id)) this.#start(id)
  }

  /** The apps that stop running take their alerts with them (there is nothing left to open). */
  forgetApps(ids: AppId[]): void {
    for (const banner of this.banners) if (ids.includes(banner.app)) this.dismiss(banner.id)
  }

  /** Does one of the sender's windows have focus here, in a place the person can see it? */
  #senderHasFocus(app: AppId): boolean {
    const { wm } = this.deps
    const win = wm.focused
    if (win?.appId !== app) return false
    return win.layout === 'slideover' ? !wm.slideOverHidden : win.stageId === wm.activeStageId
  }

  #start(id: number): void {
    this.#timers.get(id)?.()
    this.#timers.set(
      id,
      this.deps.schedule(() => this.dismiss(id), BANNER_MS)
    )
  }

  #forget(id: number): void {
    this.#timers.get(id)?.()
    this.#timers.delete(id)
  }
}

export const notifications = new Notifications()
