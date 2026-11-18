<!--
  @atom CalcDisplay — right-aligned result readout that shrinks to fit.
  Test ids: `calculator-display`, `calculator-display-caption`, `calculator-display-value`.
-->
<script lang="ts">
  interface Props {
    value: string
    /** The expression that produced the value, shown small above it. */
    caption?: string
  }

  let { value, caption }: Props = $props()

  // Shrink to fit the space the display is given: its width (digits ≈ 0.6em
  // wide in SF Pro Display Light) and its height (caption row + 1.1 line height).
  let width = $state(0)
  let height = $state(0)
  const byWidth = $derived(width ? (width - 48) / (value.length * 0.6) : 80)
  const byHeight = $derived(height ? (height - 30) / 1.1 : 80)
  const size = $derived(Math.max(28, Math.min(80, byWidth, byHeight)))
</script>

<output class="display" aria-live="polite" data-testid="calculator-display" bind:clientWidth={width} bind:clientHeight={height}>
  <span class="caption" data-testid="calculator-display-caption">{caption ?? ''}</span>
  <span class="value" data-testid="calculator-display-value" style:font-size="{size}px">{value}</span>
</output>

<style>
  .display {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    justify-content: flex-end;
    padding: 0 24px 8px;
    color: #fff;
    flex: 1 1 0;
    min-height: 0;
    overflow: hidden;
  }
  .caption {
    min-height: 22px;
    font: 400 17px/22px var(--font-text);
    color: rgb(235 235 245 / 0.6);
    white-space: nowrap;
  }
  .value {
    font-family: var(--font-display);
    font-weight: 300;
    line-height: 1.1;
    letter-spacing: -1px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    -webkit-user-select: text;
    user-select: text;
  }
</style>
