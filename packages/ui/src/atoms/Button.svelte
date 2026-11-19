<!--
  @atom Button — iOS 15+ button styles (filled / tinted / gray / plain).
  Content is a snippet; compose with <Icon> at the molecule level or above.
  Test id: `testid` on the button.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import { cssColor, inkFor } from '../contrast'

  interface Props extends HTMLButtonAttributes {
    testid: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    variant?: 'filled' | 'tinted' | 'gray' | 'plain'
    /** `fill` takes the whole box its parent gives it (a grid cell, a stretched stack slot), at least 44 pt tall. */
    size?: 'small' | 'medium' | 'large' | 'fill'
    /** Capsule (pill) corners instead of rounded rect. */
    capsule?: boolean
    /** Square button sized for a single glyph. */
    square?: boolean
    /**
     * A 44 x 44 pt hit area around a smaller drawn button: the box is padded out with a transparent border and the
     * fill stops at the padding edge, so the button looks as before but is as easy to hit as a native one.
     */
    hit?: boolean
    /** Any CSS colour; defaults to the inherited --tint. */
    tint?: string
    /**
     * A data colour (palette token or `#rrggbb`) for a `filled` button: the text colour is chosen for 4.5:1 on it.
     * Unlike `tint`, which is the accent of the control, this is content.
     */
    fill?: string
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
    tint,
    fill,
    destructive = false,
    block = false,
    type = 'button',
    children,
    class: klass = '',
    ...rest
  }: Props = $props()

  const color = $derived(destructive ? 'var(--ios-red)' : tint)
  const fillColor = $derived(fill ? cssColor(fill) : undefined)
  const ink = $derived(fill && variant === 'filled' ? inkFor(fill) : undefined)
</script>

<button
  {type}
  data-testid={testid}
  class="ios-button {variant} {size} {klass}"
  class:capsule
  class:square
  class:hit
  class:block
  class:inverts={fill && variant === 'filled'}
  style:--btn-tint={color}
  style:--btn-fill={fillColor ?? (tint && !destructive ? tint : undefined)}
  style:--btn-ink={ink}
  {...rest}
>
  {@render children()}
</button>

<style>
  .ios-button {
    --_tint: var(--btn-tint, var(--tint));
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
  .fill {
    display: flex;
    width: 100%;
    height: 100%;
    font: var(--type-body);
    font-weight: 600;
    letter-spacing: var(--tracking-body);
    min-height: 44px;
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

  /* The hit area: min 44 pt, the drawn size (28 / 34) kept inside it. */
  .hit {
    --_h: 28px;
    box-sizing: border-box;
    min-height: 44px;
    border-block: calc((44px - var(--_h)) / 2) solid transparent;
    background-clip: padding-box;
  }
  .hit.medium {
    --_h: 34px;
  }
  .hit.large {
    --_h: 50px;
    border-block-width: 0;
  }
  .hit.square {
    min-width: 44px;
    border-inline: calc((44px - var(--_w, 28px)) / 2) solid transparent;
  }
  .hit.square.medium {
    --_w: 34px;
  }
  .hit.square.small,
  .hit.square.medium {
    width: 44px;
  }
  .hit.square.large {
    width: 50px;
    border-inline-width: 0;
  }

  .filled {
    /*
     * White text on the system tint: darkened in dark mode to keep 4.5:1 (see
     * --tint-fill). An explicit `tint` is used exactly as given.
     */
    background: var(--btn-fill, light-dark(var(--_tint), color-mix(in oklab, var(--_tint) 70%, black)));
    color: var(--btn-ink, #fff);
  }
  /*
   * A pressed (`aria-pressed`) data-colour button swaps its fill and its text and gains a ring in the fill colour:
   * the pending operator of a calculator, the active toggle of a keypad. The ink was picked for 4.5:1 on the fill,
   * so the swap keeps that contrast both ways, and the ring keeps the key visible on a canvas the ink matches.
   */
  .inverts[aria-pressed='true'] {
    background: var(--btn-ink);
    color: var(--btn-fill);
    outline: 2px solid var(--btn-fill);
    outline-offset: -2px;
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
    background: color-mix(in srgb, var(--_tint) 15%, transparent);
    color: var(--_ink);
  }
  .gray {
    background: var(--fill-tertiary);
    color: var(--_ink);
  }
  .plain {
    background: transparent;
    color: var(--_tint);
    font-weight: 400;
    padding-inline: 4px;
  }
</style>
