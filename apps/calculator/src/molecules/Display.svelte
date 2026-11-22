<!-- @molecule Display — the number on the calculator and the caption over it; a long number shrinks to fit -->
<script lang="ts">
  interface Props {
    /** The number, as the backend formatted it. */
    display: string
    /** The sum so far, or the reason for an error; empty takes the same room. */
    caption: string
  }

  let { display, caption }: Props = $props()

  // the display shrinks to fit a long number in the column, as the iPadOS display does
  const size = $derived(Math.min(64, Math.floor((280 * 1.75) / Math.max(display.length, 1))))
</script>

<p class="caption" data-testid="calculator-caption">{caption}</p>
<p
  class="display"
  style:font-size="{size}px"
  aria-label="Result {display}"
  aria-live="polite"
  data-testid="calculator-display"
>
  {display}
</p>

<style>
  .caption {
    margin: 0;
    min-height: 1.4em;
    font: var(--type-body);
    color: var(--label-secondary);
    text-align: end;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .display {
    margin: 0;
    font-family: var(--font-display);
    font-weight: 300;
    line-height: 1.1;
    text-align: end;
    white-space: nowrap;
    overflow: hidden;
    font-variant-numeric: tabular-nums;
  }
</style>
