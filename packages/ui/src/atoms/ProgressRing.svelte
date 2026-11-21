<!--
  @atom ProgressRing — circular progress, determinate (`value` 0..1) or indeterminate (no value: a turning arc,
  still under Reduce Motion). role progressbar with a name; the percentage is the value text.
-->
<script lang="ts">
  interface Props {
    /** 0..1. Omit for an indeterminate ring. */
    value?: number
    size?: number
    tint?: string
    /** Accessible name. */
    label?: string
    testid?: string
  }

  let { value, size = 28, tint, label = 'Progress', testid }: Props = $props()

  const clamped = $derived(
    value === undefined || !Number.isFinite(value) ? undefined : Math.min(1, Math.max(0, value))
  )
  const stroke = $derived(Math.max(3, Math.round(size / 9)))
  const radius = $derived((size - stroke) / 2)
  const circumference = $derived(2 * Math.PI * radius)
</script>

<span
  class="ios-ring"
  class:indeterminate={clamped === undefined}
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={clamped === undefined ? undefined : Math.round(clamped * 100)}
  data-testid={testid}
  style:width="{size}px"
  style:height="{size}px"
  style:--ring-tint={tint}
>
  <svg viewBox="0 0 {size} {size}" width={size} height={size} aria-hidden="true">
    <circle class="track" cx={size / 2} cy={size / 2} r={radius} stroke-width={stroke} />
    <circle
      class="arc"
      cx={size / 2}
      cy={size / 2}
      r={radius}
      stroke-width={stroke}
      stroke-linecap="round"
      stroke-dasharray="{clamped === undefined ? circumference * 0.28 : circumference * clamped} {circumference}"
      transform="rotate(-90 {size / 2} {size / 2})"
    />
  </svg>
</span>

<style>
  .ios-ring {
    display: inline-block;
    flex: none;
    vertical-align: middle;
  }
  svg {
    display: block;
  }
  circle {
    fill: none;
  }
  .track {
    stroke: var(--fill);
  }
  .arc {
    stroke: var(--ring-tint, var(--tint));
    transition: stroke-dasharray var(--duration) var(--ease-ios);
  }
  .indeterminate svg {
    animation: turn 1s linear infinite;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
</style>
