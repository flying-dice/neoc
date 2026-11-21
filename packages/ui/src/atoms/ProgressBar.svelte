<!-- @atom ProgressBar — UIProgressView (determinate or indeterminate). -->
<script lang="ts">
  interface Props {
    /** 0–1. Omit for an indeterminate bar. */
    value?: number
    tint?: string
    label?: string
  }

  let { value, tint, label }: Props = $props()
  const clamped = $derived(value === undefined ? undefined : Math.min(1, Math.max(0, value)))
</script>

<div
  class="ios-progress"
  class:indeterminate={clamped === undefined}
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={clamped === undefined ? undefined : Math.round(clamped * 100)}
  style:--progress-tint={tint}
>
  <div class="bar" style:width={clamped === undefined ? undefined : `${clamped * 100}%`}></div>
</div>

<style>
  .ios-progress {
    position: relative;
    height: 4px;
    width: 100%;
    border-radius: 2px;
    overflow: hidden;
    background: var(--fill);
  }
  .bar {
    height: 100%;
    border-radius: 2px;
    background: var(--progress-tint, var(--tint));
    transition: width var(--duration) var(--ease-ios);
  }
  .indeterminate .bar {
    width: 35%;
    animation: slide 1.3s var(--ease-ios) infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(290%);
    }
  }
  /*
   * It says the app is busy, so it keeps going under Reduce Motion (base.css stops every other animation), but it fades
   * in place in the middle of the track instead of sliding along it (in the middle, so it never reads as a share done).
   */
  @media (prefers-reduced-motion: reduce) {
    .indeterminate .bar {
      margin-inline: auto;
      animation-name: pulse;
      animation-duration: 1.3s !important;
    }
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
