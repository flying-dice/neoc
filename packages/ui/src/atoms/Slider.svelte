<!--
  @atom Slider — UISlider-style range input (a native range input: arrows, Home/End, PageUp/PageDown work).
  `ticks` draws that many evenly spaced marks along the track. Test id: `testid`.
-->
<script lang="ts">
  import { describe } from '../aria'
  interface Props {
    testid: string
    value?: number
    min?: number
    max?: number
    step?: number
    disabled?: boolean
    tint?: string
    /** Accessible name. */
    label?: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    /** Spoken value ("40 percent") when the number alone is not enough. */
    valuetext?: string
    /** Number of tick marks (2 or more) drawn along the track. */
    ticks?: number
    oninput?: (value: number) => void
    /** The person pressed the thumb or a key (`true`), and let go or left (`false`). */
    oneditingchange?: (editing: boolean) => void
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
    'aria-description': description,
    valuetext,
    ticks = 0,
    oninput,
    oneditingchange
  }: Props = $props()

  const span = $derived(max - min)
  const pct = $derived(span > 0 ? ((value - min) / span) * 100 : 0)
  const marks = $derived(
    ticks >= 2 ? Array.from({ length: Math.min(ticks, 101) }, (_, i) => i / (Math.min(ticks, 101) - 1)) : []
  )
</script>

<div class="wrap">
  <input
    class="ios-slider"
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    aria-label={label}
    {...describe(description)}
    aria-valuetext={valuetext}
    data-testid={testid}
    bind:value
    style:--pct="{pct}%"
    style:--slider-tint={tint}
    oninput={() => oninput?.(value)}
    onpointerdown={() => oneditingchange?.(true)}
    onpointerup={() => oneditingchange?.(false)}
    onpointercancel={() => oneditingchange?.(false)}
    onfocus={(e) => e.currentTarget.matches(':focus-visible') && oneditingchange?.(true)}
    onblur={() => oneditingchange?.(false)}
  />
  {#if marks.length}
    <span class="ticks" aria-hidden="true">
      {#each marks as m (m)}<i style:left="calc(14px + (100% - 28px) * {m})"></i>{/each}
    </span>
  {/if}
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
    outline: 3px solid var(--focus-ring);
  }
  .ticks {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .ticks i {
    position: absolute;
    top: 50%;
    width: 2px;
    height: 8px;
    margin: -4px 0 0 -1px;
    border-radius: 1px;
    background: var(--label-tertiary);
  }
</style>
