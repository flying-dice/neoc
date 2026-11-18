<!-- @atom CalcKey — round calculator key (digit / function / operator). Test id: `testid`. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid: string
    variant?: 'digit' | 'function' | 'operator'
    /** Spans two columns (the 0 key). */
    wide?: boolean
    /** Highlight a pending operator. */
    active?: boolean
    label: string
    onclick?: () => void
    children: Snippet
  }

  let { testid, variant = 'digit', wide = false, active = false, label, onclick, children }: Props = $props()
</script>

<button
  type="button"
  class="key {variant}"
  data-testid={testid}
  class:wide
  class:active
  aria-label={label}
  aria-pressed={variant === 'operator' ? active : undefined}
  {onclick}
>
  {@render children()}
</button>

<style>
  .key {
    appearance: none;
    border: 0;
    width: 100%;
    height: 100%;
    border-radius: 999px;
    display: grid;
    place-items: center;
    /* Sized by the keypad: 1em = 42% of the key. */
    font: 400 1em/1 var(--font-display);
    letter-spacing: 0;
    cursor: pointer;
    transition:
      filter 120ms ease,
      background-color 200ms ease,
      color 200ms ease;
  }
  .key:active {
    filter: brightness(1.45);
  }
  .wide {
    grid-column: span 2;
    justify-items: start;
    padding-left: 0.9em;
  }
  .digit {
    background: #333335;
    color: #fff;
  }
  .function {
    background: #a5a5a7;
    color: #000;
    font-size: 0.82em;
    font-weight: 500;
  }
  .operator {
    background: #ff9f0a;
    color: #fff;
    font-size: 1.12em;
  }
  .operator.active {
    background: #fff;
    color: #ff9f0a;
  }
</style>
