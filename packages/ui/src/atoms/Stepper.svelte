<!--
  @atom Stepper — UIStepper with its value. The value is a spinbutton (the one tab stop: Up/Down step, PageUp/
  PageDown by ten steps, Home/End to the bounds); the minus and plus buttons are for pointer and touch.
  Test ids: `testid` on the spinbutton, `${testid}-decrement`, `${testid}-increment`.
-->
<script lang="ts">
  import { describe } from '../aria'
  import { clampStep } from '../stepper'

  interface Props {
    testid: string
    value?: number
    min?: number
    max?: number
    step?: number
    disabled?: boolean
    /** Accessible name (and the base of the button names: "Decrease <label>"). */
    label: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    /** Spoken and shown value when not the bare number (default: the number). */
    valuetext?: string
    onchange?: (value: number) => void
  }

  let {
    testid,
    value = $bindable(0),
    min,
    max,
    step = 1,
    disabled = false,
    label,
    'aria-description': description,
    valuetext,
    onchange
  }: Props = $props()

  let valueEl: HTMLElement | undefined = $state()
  const atMin = $derived(min !== undefined && value <= min)
  const atMax = $derived(max !== undefined && value >= max)

  function set(next: number): void {
    const v = clampStep(next, min, max, step)
    if (v === value) return
    value = v
    onchange?.(v)
  }

  function onkeydown(e: KeyboardEvent): void {
    if (disabled) return
    const big = step * 10
    const handled: Record<string, () => void> = {
      ArrowUp: () => set(value + step),
      ArrowRight: () => set(value + step),
      ArrowDown: () => set(value - step),
      ArrowLeft: () => set(value - step),
      PageUp: () => set(value + big),
      PageDown: () => set(value - big),
      Home: () => min !== undefined && set(min),
      End: () => max !== undefined && set(max)
    }
    const run = handled[e.key]
    if (!run) return
    e.preventDefault()
    run()
  }
</script>

<div class="ios-stepper" class:disabled role="group" aria-label={label}>
  <button
    type="button"
    class="side minus"
    tabindex="-1"
    data-testid="{testid}-decrement"
    aria-label="Decrease {label}"
    disabled={disabled || atMin}
    onclick={() => {
      set(value - step)
      valueEl?.focus()
    }}
  >
    <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M5 12h14" /></svg>
  </button>
  <span
    class="value"
    role="spinbutton"
    bind:this={valueEl}
    tabindex={disabled ? -1 : 0}
    data-testid={testid}
    aria-label={label}
    {...describe(description)}
    aria-valuenow={value}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuetext={valuetext}
    aria-disabled={disabled || undefined}
    {onkeydown}
  >
    {valuetext ?? value}
  </span>
  <button
    type="button"
    class="side plus"
    tabindex="-1"
    data-testid="{testid}-increment"
    aria-label="Increase {label}"
    disabled={disabled || atMax}
    onclick={() => {
      set(value + step)
      valueEl?.focus()
    }}
  >
    <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
  </button>
</div>

<style>
  .ios-stepper {
    display: inline-flex;
    align-items: stretch;
    min-height: 34px;
    border-radius: 9px;
    background: var(--fill-tertiary);
    overflow: hidden;
  }
  .disabled {
    opacity: 0.4;
  }
  .side {
    appearance: none;
    border: 0;
    background: transparent;
    display: grid;
    place-items: center;
    width: 44px;
    padding: 0;
    color: var(--label);
    cursor: pointer;
  }
  .side:disabled {
    color: var(--label-tertiary);
    cursor: default;
  }
  .side:active:not(:disabled) {
    background: var(--fill-secondary);
  }
  .value {
    display: grid;
    place-items: center;
    min-width: 48px;
    padding: 0 8px;
    font: var(--type-body);
    font-variant-numeric: tabular-nums;
    border-inline: var(--hairline) solid var(--separator);
    color: var(--label);
  }
  .value:focus-visible {
    outline-offset: -3px;
  }
</style>
