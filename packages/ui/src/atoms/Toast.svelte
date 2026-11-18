<!--
  @atom Toast — a transient message on an elevated card: a leading glyph (a snippet, coloured by `kind`), the
  text, an optional action button and a dismiss button. Errors are `alert`s, the rest polite `status` messages.
  While the pointer or keyboard focus is on the toast it reports `onhold`, and `onrelease` when both have left, so
  the owner can pause its expiry (WCAG 2.2.1). The buttons are at least 44x44.
  Test ids: `testid` on the toast, `${testid}-action`, `${testid}-dismiss`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid: string
    kind?: 'info' | 'success' | 'error' | 'conflict'
    text: string
    /** Label of the action button (omit for none). */
    action?: string
    /** The glyph; render a kit `Icon` here. Decorative: the text carries the meaning. */
    icon?: Snippet
    onaction?: () => void
    ondismiss?: () => void
    /** The pointer or focus arrived on the toast: do not expire it while it is read. */
    onhold?: () => void
    /** The pointer and focus have both left. */
    onrelease?: () => void
  }

  let { testid, kind = 'info', text, action, icon, onaction, ondismiss, onhold, onrelease }: Props =
    $props()

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
  class="ios-toast {kind}"
  role={kind === 'error' ? 'alert' : 'status'}
  aria-atomic="true"
  data-testid={testid}
  onpointerenter={() => track(() => (hovered = true))}
  onpointerleave={() => track(() => (hovered = false))}
  onfocusin={() => track(() => (focused = true))}
  onfocusout={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) track(() => (focused = false))
  }}
>
  {#if icon}<span class="glyph" aria-hidden="true">{@render icon()}</span>{/if}
  <span class="text">{text}</span>
  {#if action}
    <button type="button" class="action" data-testid="{testid}-action" onclick={() => onaction?.()}>{action}</button>
  {/if}
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
  .glyph {
    display: grid;
    flex: none;
    color: var(--tint);
  }
  .success .glyph {
    color: var(--ios-green);
  }
  .error .glyph {
    color: var(--ios-red);
  }
  .conflict .glyph {
    color: var(--ios-orange);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  button {
    appearance: none;
    border: 0;
    background: transparent;
    cursor: pointer;
    color: var(--tint);
    font: inherit;
    font-weight: 600;
    min-height: 44px;
    min-width: 44px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
  }
  .dismiss {
    display: grid;
    place-items: center;
    padding: 0;
    color: var(--label-secondary);
  }
  button:active {
    background: var(--fill-secondary);
  }
</style>
