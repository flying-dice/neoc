<!--
  @atom Toast — a transient message on an elevated card: the text and a dismiss button. A polite `status` message.
  While the pointer or keyboard focus is on the toast it reports `onhold`, and `onrelease` when both have left, so
  the owner can pause its expiry (WCAG 2.2.1). The button is at least 44x44.
  Test ids: `testid` on the toast, `${testid}-dismiss`.
-->
<script lang="ts">
  interface Props {
    testid: string
    text: string
    /** The toast is its own live region (the default). False when the owner announces the text elsewhere, in a region it keeps mounted. */
    live?: boolean
    ondismiss?: () => void
    /** The pointer or focus arrived on the toast: do not expire it while it is read. */
    onhold?: () => void
    /** The pointer and focus have both left. */
    onrelease?: () => void
  }

  let { testid, text, live = true, ondismiss, onhold, onrelease }: Props = $props()

  let hovered = false
  let focused = false
  function track(change: () => void): void {
    const was = hovered || focused
    change()
    const now = hovered || focused
    if (now && !was) onhold?.()
    else if (!now && was) onrelease?.()
  }
</script>

<!-- A status message with no behaviour of its own: the handlers only pause its expiry while it is read. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="ios-toast"
  role={live ? 'status' : undefined}
  aria-atomic={live ? 'true' : undefined}
  data-testid={testid}
  onpointerenter={() => track(() => (hovered = true))}
  onpointerleave={() => track(() => (hovered = false))}
  onfocusin={() => track(() => (focused = true))}
  onfocusout={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) track(() => (focused = false))
  }}
>
  <span class="text">{text}</span>
  <button type="button" class="dismiss" data-testid="{testid}-dismiss" aria-label="Dismiss" onclick={() => ondismiss?.()}>
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
  </button>
</div>

<style>
  .ios-toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 6px 6px 6px 14px;
    border-radius: var(--radius-lg);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    border: var(--hairline) solid var(--separator);
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .dismiss {
    appearance: none;
    border: 0;
    background: transparent;
    cursor: pointer;
    display: grid;
    place-items: center;
    min-height: 44px;
    min-width: 44px;
    padding: 0;
    border-radius: var(--radius-sm);
    color: var(--label-secondary);
  }
  .dismiss:active {
    background: var(--fill-secondary);
  }
</style>
