<!-- @atom Slider — UISlider-style range input. Test id: `testid`. -->
<script lang="ts">
  interface Props {
    testid: string
    value?: number
    min?: number
    max?: number
    step?: number
    disabled?: boolean
    tint?: string
    label?: string
    oninput?: (value: number) => void
  }

  let {
    testid,
    value = $bindable(50),
    min = 0,
    max = 100,
    step = 1,
    disabled = false,
    tint,
    label,
    oninput
  }: Props = $props()

  const pct = $derived(((value - min) / (max - min)) * 100)
</script>

<input
  class="ios-slider"
  type="range"
  {min}
  {max}
  {step}
  {disabled}
  aria-label={label}
  data-testid={testid}
  bind:value
  style:--pct="{pct}%"
  style:--slider-tint={tint}
  oninput={() => oninput?.(value)}
/>

<style>
  .ios-slider {
    flex: 1;
    width: 100%;
    min-width: 60px;
    appearance: none;
    -webkit-appearance: none;
    height: 28px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  .ios-slider:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .ios-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--slider-tint, var(--tint)) var(--pct),
      var(--fill) var(--pct)
    );
  }
  .ios-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 28px;
    height: 28px;
    margin-top: -12px;
    border-radius: 50%;
    background: var(--knob);
    box-shadow:
      0 0.5px 4px rgb(0 0 0 / 0.12),
      0 6px 13px rgb(0 0 0 / 0.12),
      0 0 0 0.5px rgb(0 0 0 / 0.04);
  }
  .ios-slider:focus-visible {
    outline: none;
  }
  .ios-slider:focus-visible::-webkit-slider-thumb {
    outline: 3px solid color-mix(in srgb, var(--tint) 55%, transparent);
  }
</style>
