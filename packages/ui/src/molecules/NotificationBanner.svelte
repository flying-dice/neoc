<!--
  @molecule NotificationBanner — a system alert: the sender's app icon and name, a one-line title and a short body, on
  an elevated card. Selecting it (click, Enter) opens the sender; the dismiss button (and Escape while focus is on the
  banner) closes it. The owner passes text that is already safe to show as one line and a few lines (the shell cleans
  what an app wrote); both are drawn as text, each in its own direction (`dir="auto"`, isolated from the app name and
  from each other), so a right-to-left title cannot reorder what is beside it. Not a live region: the owner announces
  the text in a region it keeps mounted. While the pointer or keyboard focus is on the banner it reports `onhold`, and
  `onrelease` when both have left, so the owner can pause its expiry (WCAG 2.2.1). The buttons are at least 44x44.
  Atoms: IconTile, Icon. Test ids: `testid` on the banner, `${testid}-open`, `${testid}-dismiss`.
-->
<script lang="ts">
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
    onselect,
    ondismiss,
    onhold,
    onrelease
  }: Props = $props()

  let hovered = false
  let focused = false
  function track(change: () => void): void {
    const was = hovered || focused
    change()
    const now = hovered || focused
    if (now && !was) onhold?.()
    else if (!now && was) onrelease?.()
  }

  const label = $derived(`Open ${appName}: ${title}${body ? `. ${body}` : ''}`)
</script>

<!-- A card with no behaviour of its own: the handlers pause its expiry while it is read, and Escape closes it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="ios-notification"
  role="group"
  aria-label="Notification from {appName}"
  data-testid={testid}
  onpointerenter={() => track(() => (hovered = true))}
  onpointerleave={() => track(() => (hovered = false))}
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
  <button
    type="button"
    class="open"
    aria-label={label}
    data-testid="{testid}-open"
    onclick={() => onselect?.()}
  >
    <span class="icon" aria-hidden="true">
      <IconTile size={40} {background} shadow>
        <Icon name={icon} size={22} weight={1.9} color="#fff" />
      </IconTile>
    </span>
    <span class="copy" aria-hidden="true">
      <span class="app">{appName}</span>
      <span class="title" dir="auto">{title}</span>
      {#if body}<span class="body" dir="auto">{body}</span>{/if}
    </span>
  </button>
  <button type="button" class="dismiss" data-testid="{testid}-dismiss" aria-label="Dismiss notification from {appName}" onclick={() => ondismiss?.()}>
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
  </button>
</div>

<style>
  .ios-notification {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    min-height: 64px;
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    border: var(--hairline) solid var(--separator);
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
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
  .open {
    flex: 1;
    min-width: 0;
    min-height: 64px;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 4px 12px 12px;
    text-align: start;
    border-radius: var(--radius-lg);
  }
  .open:active,
  .dismiss:active {
    background: var(--fill-secondary);
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
  .app {
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
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin: 4px 4px 0 0;
    color: var(--label-secondary);
    border-radius: var(--radius-sm);
  }
</style>
