<!--
  @molecule NotificationBanner — a system alert: the sender's app icon and name, a one-line title and a short body, on
  an elevated card. Selecting it (click, Enter) opens the sender; the dismiss button (and Escape while focus is on the
  banner) closes it. The owner passes text that is already safe to show as one line and a few lines (the shell cleans
  what an app wrote); both are drawn as text, each in its own direction (`dir="auto"`, isolated from the app name and
  from each other), so a right-to-left title cannot reorder what is beside it. Not a live region: the owner announces
  the text in a region it keeps mounted. The control that opens the sender is named by the sender and the title; the
  body is its description (`spokenBody`, the start of it, when the owner has one: the body can be two thousand
  characters and the card shows a few lines). While the keyboard focus is on the banner, or the pointer is being moved
  over it, it reports `onhold`, and `onrelease` when both have stopped, so the owner can pause its expiry (WCAG 2.2.1):
  a pointer that merely rests where the banner appeared holds nothing, or a banner would stay for as long as the mouse
  does. The buttons are at least 44x44.
  Atoms: IconTile, Icon. Test ids: `testid` on the banner, `${testid}-open`, `${testid}-dismiss`.
-->
<script lang="ts">
  import { onDestroy } from 'svelte'
  import IconTile from '../atoms/IconTile.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    /** The sender's name, from the host's list of apps. */
    appName: string
    icon: IconName
    /** Any CSS background for the icon plate (the app's tint). */
    background?: string
    /** One line: longer text is cut with an ellipsis. */
    title: string
    /** Up to a few lines; line breaks are kept. */
    body?: string
    /** What a screen reader gets as the description of the control: the start of `body`. Default: all of it. */
    spokenBody?: string
    onselect?: () => void
    ondismiss?: () => void
    /** The pointer or focus arrived on the banner: do not expire it while it is read. */
    onhold?: () => void
    /** The pointer and focus have both left. */
    onrelease?: () => void
  }

  let {
    testid,
    appName,
    icon,
    background = 'var(--ios-gray)',
    title,
    body,
    spokenBody,
    onselect,
    ondismiss,
    onhold,
    onrelease
  }: Props = $props()

  /** The pointer counts as in use for this long after it last moved over the banner. */
  const POINTER_IDLE_MS = 2000

  let moving = false
  let focused = false
  let idle: ReturnType<typeof setTimeout> | undefined
  const reading = (): boolean => moving || focused
  function track(change: () => void): void {
    const was = reading()
    change()
    const now = reading()
    if (now && !was) onhold?.()
    else if (!now && was) onrelease?.()
  }
  function pointerMoved(e: PointerEvent): void {
    // a pointer that is still reports nothing, but the browser repeats its position when the page changes under it
    if (!e.movementX && !e.movementY) return
    clearTimeout(idle)
    idle = setTimeout(() => track(() => (moving = false)), POINTER_IDLE_MS)
    track(() => (moving = true))
  }
  function pointerLeft(): void {
    clearTimeout(idle)
    track(() => (moving = false))
  }
  onDestroy(() => clearTimeout(idle))

  const uid = $props.id()
  const label = $derived(`Open ${appName}: ${title}`)
  const description = $derived(body ? (spokenBody ?? body) : undefined)
</script>

<!-- A card with no behaviour of its own: the handlers pause its expiry while it is read, and Escape closes it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="ios-notification"
  role="group"
  aria-label="Notification from {appName}"
  data-testid={testid}
  onpointermove={pointerMoved}
  onpointerleave={pointerLeft}
  onfocusin={() => track(() => (focused = true))}
  onfocusout={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) track(() => (focused = false))
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      ondismiss?.()
    }
  }}
>
  <span class="icon" aria-hidden="true">
    <IconTile size={40} {background} shadow>
      <Icon name={icon} size={22} weight={1.9} color="#fff" />
    </IconTile>
  </span>
  <div class="copy">
    <!-- The control is the sender and the title; it reaches over the whole card (the `::after` of `.open`), so the body
         below is part of what a click opens. The body is not inside it: the name must hold the text the button shows. -->
    <button
      type="button"
      class="open"
      aria-label={label}
      aria-describedby={description ? `${uid}-description` : undefined}
      data-testid="{testid}-open"
      onclick={() => onselect?.()}
    >
      <span class="app" dir="auto">{appName}</span>
      <span class="title" dir="auto">{title}</span>
    </button>
    {#if body}<span class="body" dir="auto" aria-hidden="true">{body}</span>{/if}
  </div>
  <!-- what a screen reader is given of the body: the start of it, as the control's description. Hidden from the tree so
       browse mode does not read it a second time; `aria-describedby` still reaches it. -->
  {#if description}<span id="{uid}-description" class="sr-only" aria-hidden="true">{description}</span>{/if}
  <button type="button" class="dismiss" data-testid="{testid}-dismiss" aria-label="Dismiss notification from {appName}" onclick={() => ondismiss?.()}>
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
  </button>
</div>

<style>
  .ios-notification {
    position: relative;
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    min-height: 64px;
    padding: 12px 0 12px 12px;
    box-sizing: border-box;
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    border: var(--hairline) solid var(--separator);
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
  }
  /* the ring goes round the card, which is what the control stands for */
  .ios-notification:has(.open:focus-visible) {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }
  button {
    appearance: none;
    border: 0;
    background: transparent;
    cursor: pointer;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
  }
  .icon {
    flex: none;
    display: grid;
  }
  .copy {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .open {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    padding: 0;
    text-align: start;
  }
  .open:focus-visible {
    outline: none;
  }
  /* over the whole card, under the dismiss button */
  .open::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: var(--radius-lg);
  }
  .open:active::after {
    background: var(--fill-secondary);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .app {
    unicode-bidi: isolate;
    font: var(--type-caption1);
    color: var(--label-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    unicode-bidi: isolate;
  }
  .body {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    overflow: hidden;
    overflow-wrap: anywhere;
    white-space: pre-line;
    unicode-bidi: isolate;
  }
  .dismiss {
    position: relative; /* above the control's reach over the card */
    z-index: 1;
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin: -8px 4px -8px 0;
    color: var(--label-secondary);
    border-radius: var(--radius-sm);
  }
  .dismiss:active {
    background: var(--fill-secondary);
  }
</style>
