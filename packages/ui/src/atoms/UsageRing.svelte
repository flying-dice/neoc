<!-- @atom UsageRing — tiny ring showing how full the agent's context window is. -->
<script lang="ts">
  interface Props {
    used: number
    size: number
    /** Optional spend for the session so far. */
    cost?: { amount: number; currency: string }
  }

  let { used, size, cost }: Props = $props()

  const fraction = $derived(size > 0 ? Math.min(1, used / size) : 0)
  const percent = $derived(Math.round(fraction * 100))
  const C = 2 * Math.PI * 6
  const fmt = (n: number): string => (n >= 1000 ? `${Math.round(n / 1000)}k` : String(n))
  const detail = $derived(
    `${fmt(used)} of ${fmt(size)} tokens` +
      (cost
        ? ` · ${new Intl.NumberFormat(undefined, { style: 'currency', currency: cost.currency }).format(cost.amount)}`
        : '')
  )
</script>

<span
  class="usage"
  class:high={fraction > 0.8}
  title="Context: {detail}"
  role="img"
  aria-label="Context {percent}% used, {detail}"
>
  <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
    <circle cx="8" cy="8" r="6" class="track" />
    <circle
      cx="8"
      cy="8"
      r="6"
      class="fill"
      stroke-dasharray="{C * fraction} {C}"
      transform="rotate(-90 8 8)"
    />
  </svg>
  <span class="pct">{percent}%</span>
</span>

<style>
  .usage {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font: var(--type-caption1);
    font-variant-numeric: tabular-nums;
    color: var(--label-secondary);
  }
  circle {
    fill: none;
    stroke-width: 2.4;
  }
  .track {
    stroke: var(--fill);
  }
  .fill {
    stroke: var(--label-secondary);
    stroke-linecap: round;
    transition: stroke-dasharray var(--duration) var(--ease-ios);
  }
  .high .fill {
    stroke: var(--ios-orange);
  }
</style>
