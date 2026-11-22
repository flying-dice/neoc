<!--
  @atom Button — iOS 15+ button styles (filled / tinted / gray / plain).
  Content is a snippet; compose with <Icon> at the molecule level or above.
  Test id: `testid` on the button.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  interface Props extends HTMLButtonAttributes {
    testid: string
    variant?: 'filled' | 'tinted' | 'gray' | 'plain'
    size?: 'small' | 'medium' | 'large'
    /** Capsule (pill) corners instead of rounded rect. */
    capsule?: boolean
    /** Square button sized for a single glyph. */
    square?: boolean
    /**
     * A 44 x 44 pt hit area around a smaller drawn button: the box is padded out with a transparent border and the
     * fill stops at the padding edge, so the button looks as before but is as easy to hit as a native one.
     */
    hit?: boolean
    destructive?: boolean
    block?: boolean
    children: Snippet
  }

  let {
    testid,
    variant = 'filled',
    size = 'medium',
    capsule = false,
    square = false,
    hit = false,
    destructive = false,
    block = false,
    type = 'button',
    children,
    ...rest
  }: Props = $props()

</script>

<button
  {type}
  data-testid={testid}
  class="ios-button {variant} {size}"
  class:capsule
  class:square
  class:hit
  class:block
  class:destructive
  {...rest}
>
  {@render children()}
</button>

<style>
  .ios-button {
    --_tint: var(--tint);
    appearance: none;
    border: 0;
    margin: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    cursor: pointer;
    white-space: nowrap;
    font-weight: 600;
    transition:
      opacity var(--duration-fast) ease,
      background-color var(--duration-fast) ease;
  }
  .ios-button:active:not(:disabled) {
    opacity: 0.6;
  }
  .ios-button:disabled {
    cursor: default;
    opacity: 0.35;
  }
  .destructive {
    --_tint: var(--ios-red);
  }
  .block {
    display: flex;
    width: 100%;
  }

  .small {
    font: var(--type-subheadline);
    font-weight: 600;
    min-height: 28px;
    padding: 0 12px;
    border-radius: 7px;
  }
  .medium {
    font: var(--type-subheadline);
    font-weight: 600;
    min-height: 34px;
    padding: 0 14px;
    border-radius: 8px;
  }
  .large {
    font: var(--type-body);
    font-weight: 600;
    letter-spacing: var(--tracking-body);
    min-height: 50px;
    padding: 0 20px;
    border-radius: 12px;
  }
  .capsule {
    border-radius: 999px;
  }
  .square {
    padding: 0;
  }
  .square.small {
    width: 28px;
  }
  .square.medium {
    width: 34px;
  }
  .square.large {
    width: 50px;
  }

  /*
   * The hit area: min 44 pt, the drawn size (28 / 34) kept inside it. The fill is painted by a pseudo-element of the
   * drawn size with the button's own radius, so the corners are the same as on a button without a hit area.
   */
  .hit {
    --_h: 28px;
    position: relative;
    isolation: isolate;
    box-sizing: border-box;
    min-height: 44px;
    background-color: transparent;
  }
  .hit::before {
    content: '';
    position: absolute;
    z-index: -1;
    inset-block: calc((100% - var(--_h)) / 2);
    inset-inline: 0;
    border-radius: inherit;
    background-color: var(--_bg, transparent);
  }
  .hit.medium {
    --_h: 34px;
  }
  .hit.large {
    --_h: 50px;
    min-height: 50px;
  }
  .hit.square {
    min-width: 44px;
  }
  .hit.square::before {
    inset-inline: calc((100% - var(--_w, 28px)) / 2);
  }
  .hit.square.medium {
    --_w: 34px;
  }
  .hit.square.small,
  .hit.square.medium {
    width: 44px;
  }
  .hit.square.large {
    --_w: 50px;
    width: 50px;
  }

  .filled {
    /* White text on the system tint: darkened in dark mode to keep 4.5:1 (see --tint-fill). */
    --_bg: light-dark(var(--_tint), color-mix(in oklab, var(--_tint) 70%, black));
    color: #fff;
  }
  /*
   * Tint-coloured text on a tinted or gray fill: darker in light mode, lighter
   * in dark, so it keeps 4.5:1 on the fill (the plain tint is ~4:1 there).
   */
  .tinted,
  .gray {
    --_ink: light-dark(color-mix(in oklab, var(--_tint) 78%, black), color-mix(in oklab, var(--_tint) 78%, white));
  }
  .tinted {
    --_bg: color-mix(in srgb, var(--_tint) 15%, transparent);
    color: var(--_ink);
  }
  .gray {
    --_bg: var(--fill-tertiary);
    color: var(--_ink);
  }
  .ios-button:not(.hit) {
    background-color: var(--_bg, transparent);
  }
  .plain {
    color: var(--_tint);
    font-weight: 400;
    padding-inline: 4px;
  }
</style>
