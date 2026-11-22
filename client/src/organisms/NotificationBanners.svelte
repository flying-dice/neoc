<!--
  @organism NotificationBanners — the system alerts of this client: the stack of banners at the top of the desktop,
  above every window, and the live region that announces them. Each banner is a kit NotificationBanner; the state
  (what is shown, for how long, what choosing one does) is the `Notifications` object it is given.
  The region is always mounted and only its contents change (a region inserted with its text already in it is often not
  announced); the banners are not live regions themselves. Alt+Shift+N moves focus to the newest banner (Tab from the
  top of the page reaches it too, as the stack comes first in it); Escape or the dismiss button closes the focused banner and focus goes back
  to where it was. Nothing moves or fades: a banner appears and goes (Reduce Motion needs no special case).
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
  }

  let { center, height = $bindable(0) }: Props = $props()

  let stack: HTMLElement | undefined = $state()
  /** What had focus before a keyboard user went to the banners, to give it back. */
  let returnTo: HTMLElement | null = null

  const focusedHere = (): boolean => !!stack && stack.contains(document.activeElement)

  /** Alt+Shift+N: not a letter on its own and not a browser or screen reader command; `code`, because Option+N is a dead key on a Mac. */
  function onkeydown(e: KeyboardEvent): void {
    if (e.code !== 'KeyN' || !e.altKey || !e.shiftKey || e.ctrlKey || e.metaKey) return
    const newest = center.banners[0]
    if (!newest) return
    e.preventDefault()
    if (!focusedHere())
      returnTo = document.activeElement instanceof HTMLElement ? document.activeElement : null
    // a key was pressed, so the ring shows even when the last thing the person did was a click
    const options: FocusOptions & { focusVisible: boolean } = { focusVisible: true }
    stack
      ?.querySelector<HTMLElement>(`[data-testid="notification-${newest.id}-open"]`)
      ?.focus(options)
  }

  $effect(() => {
    if (!center.banners.length) height = 0 // the stack is gone: nothing sits below it any more
  })

  /** Dismiss one; if the keyboard was on the banners it goes back to where it came from. */
  async function dismiss(id: number): Promise<void> {
    const wasFocused = focusedHere()
    center.dismiss(id)
    if (!wasFocused) return
    await tick()
    if (returnTo?.isConnected) returnTo.focus()
    returnTo = null
  }

  /** Open the sender's window and put the keys there: the banner that had them is gone. */
  async function open(id: number): Promise<void> {
    const app = center.banners.find((b) => b.id === id)?.app
    center.select(id)
    returnTo = null
    await tick()
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
    <p>{banner.appName}: {banner.title}{banner.body ? `. ${banner.body}` : ''}</p>
  {/each}
</div>

{#if center.banners.length}
  <section
    class="stack"
    aria-label="Notifications"
    data-testid="notifications"
    bind:this={stack}
    bind:clientHeight={height}
  >
    {#each center.banners as banner (banner.id)}
      <NotificationBanner
        testid="notification-{banner.id}"
        appName={banner.appName}
        icon={banner.icon}
        background={banner.background}
        title={banner.title}
        body={banner.body}
        onselect={() => void open(banner.id)}
        ondismiss={() => void dismiss(banner.id)}
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
