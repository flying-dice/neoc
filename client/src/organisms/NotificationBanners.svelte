<!--
  @organism NotificationBanners — the system alerts of this client: the stack of banners at the top of the desktop,
  above every window, and the live region that announces them. Each banner is a kit NotificationBanner; the state
  (what is shown, for how long, what choosing one does) is the `Notifications` object it is given.
  The region is always mounted and only its contents change (a region inserted with its text already in it is often not
  announced); the banners are not live regions themselves, and what it says of each is the sender, the title and the
  start of the body. Alt+Shift+N (not while focus is in a field that takes typing: it is a character on a Mac keyboard)
  moves focus to the newest banner (the stack also comes first in the page's order, so Tab
  from the very top, or Shift+Tab back out of a window, reaches it); Escape or the dismiss button closes the focused banner.
  Focus is never dropped to the page: when the banner that has it goes (dismissed, expired, pushed out by newer ones,
  its app removed) it moves to the nearest banner that is left, else back to where the person was before they went to
  the banners, else to `fallback` (the front window, or the Dock). Nothing moves or fades: a banner appears and goes
  (Reduce Motion needs no special case).
  Test ids: `notifications` (the stack), `notifications-live`, `notification-<n>` (+ `-open`, `-dismiss`, see
  NotificationBanner).
-->
<script lang="ts">
  import { tick } from 'svelte'
  import { NotificationBanner } from '@vos/ui/molecules'
  import type { Notifications } from '../lib/notifications.svelte'

  interface Props {
    center: Notifications
    /** The height the stack takes, for what else sits at the top of the desktop (a notice moves down by it). */
    height?: number
    /** Where focus goes when the banners had it and nothing better is left: the front window, or the Dock. */
    fallback?: () => HTMLElement | null | undefined
  }

  let { center, height = $bindable(0), fallback }: Props = $props()

  let stack: HTMLElement | undefined = $state()
  /** What had focus before a keyboard user went to the banners, to give it back. */
  let returnTo: HTMLElement | null = null
  /** The banners on screen at the last look, newest first: where a banner stood tells which one is next to it. */
  let seen: number[] = []
  /** The banner that has focus just before the stack changes (read before the page is updated). */
  let focusedBefore: { id: number; ring: boolean } | null = null
  /** The person chose a banner: the window it opened takes the keys, not another banner. */
  let opening = false

  const idOf = (el: Element | null): number | null => {
    const match = /^notification-(\d+)(?:-open|-dismiss)?$/.exec(
      el?.closest('[data-testid^="notification-"]')?.getAttribute('data-testid') ?? ''
    )
    return match ? Number(match[1]) : null
  }
  const openControl = (id: number): HTMLElement | null =>
    stack?.querySelector<HTMLElement>(`[data-testid="notification-${id}-open"]`) ?? null
  const focusVisibly = (el: HTMLElement, ring: boolean): void => {
    // a key was pressed, so the ring shows even when the last thing the person did was a click
    const options = { focusVisible: ring } as unknown as Parameters<HTMLElement['focus']>[0]
    el.focus(options)
  }

  /** A field that takes typed text (or something inside one): a text input, a text area, a select, editable content. */
  const EDITABLE = [
    'input:not([type="button" i]):not([type="checkbox" i]):not([type="radio" i]):not([type="range" i])',
    ':not([type="submit" i]):not([type="reset" i]):not([type="file" i]):not([type="color" i]):not([type="image" i])',
    ', textarea, select, [contenteditable]:not([contenteditable="false" i])'
  ].join('')
  const typing = (e: KeyboardEvent): boolean => {
    const el = e.target instanceof Element ? e.target : document.activeElement
    return (
      !!el && (el.closest(EDITABLE) !== null || (el instanceof HTMLElement && el.isContentEditable))
    )
  }

  /**
   * Alt+Shift+N: not a letter on its own and not a browser or screen reader command; `code`, because Option+N is a
   * dead key on a Mac. Not while typing: Option+Shift+N is a character on a Mac keyboard ("˜"), and a shortcut must not
   * eat what a person types. From a text field, Tab out first.
   */
  function onkeydown(e: KeyboardEvent): void {
    if (e.code !== 'KeyN' || !e.altKey || !e.shiftKey || e.ctrlKey || e.metaKey) return
    if (typing(e)) return
    const newest = center.banners[0]
    const control = newest ? openControl(newest.id) : null
    if (!control) return
    e.preventDefault()
    if (!stack?.contains(document.activeElement)) {
      returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null
    }
    focusVisibly(control, true)
  }

  // before the page changes with the banners: who has the keys now?
  $effect.pre(() => {
    void center.banners
    const active = document.activeElement
    const id = active instanceof HTMLElement && stack?.contains(active) ? idOf(active) : null
    focusedBefore =
      id === null ? null : { id, ring: (active as HTMLElement).matches(':focus-visible') }
  })

  // after it: if the banner that had them is gone, they go on to the nearest one, else back where they were
  $effect(() => {
    const ids = center.banners.map((b) => b.id)
    const lost = focusedBefore
    const before = seen
    seen = ids
    if (!ids.length) height = 0 // the stack is gone: nothing sits below it any more
    if (!lost || ids.includes(lost.id) || opening) return
    const active = document.activeElement
    if (active && active !== document.body && active.isConnected) return // they went somewhere on their own
    const at = before.indexOf(lost.id)
    const older = before.slice(at + 1).find((id) => ids.includes(id))
    const newer = before
      .slice(0, Math.max(at, 0))
      .reverse()
      .find((id) => ids.includes(id))
    const next = older ?? newer
    const target =
      (next === undefined ? null : openControl(next)) ??
      (returnTo?.isConnected ? returnTo : null) ??
      fallback?.() ??
      null
    returnTo = null
    if (target) focusVisibly(target, lost.ring)
  })

  /** Open the sender's window and put the keys there: the banner that had them is gone. */
  async function open(id: number): Promise<void> {
    const app = center.banners.find((b) => b.id === id)?.app
    opening = true
    center.select(id)
    returnTo = null
    await tick()
    opening = false
    if (app) document.querySelector<HTMLElement>(`[data-testid="window-${app}"]`)?.focus()
  }
</script>

<svelte:window {onkeydown} />

<div
  class="sr-only"
  role="status"
  aria-live="polite"
  aria-relevant="additions text"
  data-testid="notifications-live"
>
  {#each center.banners as banner (banner.id)}
    <p>{banner.announcement}</p>
  {/each}
</div>

{#if center.banners.length}
  <section
    class="stack"
    aria-label="Notifications"
    data-testid="notifications"
    bind:this={stack}
    bind:clientHeight={height}
    onfocusout={(e) => {
      // the keys went to something else on purpose: there is nothing to give back
      if (e.relatedTarget instanceof Node && !e.currentTarget.contains(e.relatedTarget))
        returnTo = null
    }}
  >
    {#each center.banners as banner (banner.id)}
      <NotificationBanner
        testid="notification-{banner.id}"
        appName={banner.appName}
        icon={banner.icon}
        background={banner.background}
        title={banner.title}
        body={banner.body}
        spokenBody={banner.spokenBody}
        onselect={() => void open(banner.id)}
        ondismiss={() => center.dismiss(banner.id)}
        onhold={() => center.hold(banner.id)}
        onrelease={() => center.release(banner.id)}
      />
    {/each}
  </section>
{/if}

<style>
  .stack {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    width: min(420px, calc(100% - 16px));
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 10002; /* above the windows, Slide Over and a notice */
    pointer-events: none; /* only the banners take the pointer, not the gaps between them */
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
