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
    /** Any CSS colour; defaults to the inherited --tint. */
    tint?: string
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
    tint,
    destructive = false,
    block = false,
    type = 'button',
    children,
    class: klass = '',
    ...rest
  }: Props = $props()

  const color = $derived(destructive ? 'var(--ios-red)' : tint)
</script>

<button
  {type}
  data-testid={testid}
  class="ios-button {variant} {size} {klass}"
  class:capsule
  class:square
  class:block
  style:--btn-tint={color}
  style:--btn-fill={tint && !destructive ? tint : undefined}
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

  .filled {
    /*
     * White text on the system tint: darkened in dark mode to keep 4.5:1 (see
     * --tint-fill). An explicit `tint` is used exactly as given.
     */
    background: var(--btn-fill, light-dark(var(--_tint), color-mix(in oklab, var(--_tint) 70%, black)));
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
