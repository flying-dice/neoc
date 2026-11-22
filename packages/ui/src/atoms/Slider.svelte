<!--
  @atom Slider — UISlider-style range input (a native range input: arrows, Home/End, PageUp/PageDown work).
  Test id: `testid`.
-->
<script lang="ts">
  interface Props {
    testid: string
    value?: number
    min?: number
    max?: number
    step?: number
    /** Accessible name. */
    label?: string
  }

  let { testid, value = $bindable(50), min = 0, max = 100, step = 1, label }: Props = $props()

  const span = $derived(max - min)
  const pct = $derived(span > 0 ? ((value - min) / span) * 100 : 0)
</script>

<div class="wrap">
  <input
    class="ios-slider"
    type="range"
    {min}
    {max}
    {step}
    aria-label={label}
    data-testid={testid}
    bind:value
    style:--pct="{pct}%"
  />
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    min-width: 60px;
  }
  .ios-slider {
    flex: 1;
    width: 100%;
    min-width: 0;
    appearance: none;
    -webkit-appearance: none;
    height: 28px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  .ios-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--tint) var(--pct),
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
    outline: 3px solid var(--focus-ring);
  }
</style>
