<!--
  @atom SegmentedControl — UISegmentedControl.
  Test ids: `testid` on the group, `${testid}-${segment.value}` on each segment.
-->
<script lang="ts" generics="T extends string">
  interface Segment {
    value: T
    label: string
  }

  interface Props {
    testid: string
    segments: Segment[]
    value?: T
    block?: boolean
    label?: string
    onchange?: (value: T) => void
  }

  let { testid, segments, value = $bindable(), block = false, label, onchange }: Props = $props()

  const index = $derived(Math.max(0, segments.findIndex((s) => s.value === value)))

  function select(v: T): void {
    value = v
    onchange?.(v)
  }
</script>

<div
  class="ios-segmented"
  class:block
  role="radiogroup"
  data-testid={testid}
  aria-label={label}
  style:--count={segments.length}
  style:--index={index}
>
  <span class="thumb" aria-hidden="true"></span>
  {#each segments as segment, i (segment.value)}
    <button
      type="button"
      role="radio"
      data-testid="{testid}-{segment.value}"
      aria-checked={segment.value === value}
      class="segment"
      class:selected={segment.value === value}
      class:hide-divider={i === index || i === index + 1 || i === 0}
      onclick={() => select(segment.value)}
    >
      {segment.label}
    </button>
  {/each}
</div>

<style>
  .ios-segmented {
    position: relative;
    display: inline-grid;
    grid-template-columns: repeat(var(--count), minmax(0, 1fr));
    padding: 2px;
    min-height: 32px;
    border-radius: 9px;
    background: var(--fill-tertiary);
    isolation: isolate;
  }
  .block {
    display: grid;
    width: 100%;
  }
  .thumb {
    position: absolute;
    z-index: 0;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc((100% - 4px) / var(--count));
    border-radius: 7px;
    background: var(--segment-thumb);
    box-shadow:
      0 3px 8px rgb(0 0 0 / 0.12),
      0 3px 1px rgb(0 0 0 / 0.04),
      0 0 0 0.5px rgb(0 0 0 / 0.04);
    transform: translateX(calc(100% * var(--index)));
    transition: transform var(--duration) var(--ease-ios);
  }
  .segment {
    appearance: none;
    border: 0;
    background: none;
    position: relative;
    z-index: 1;
    padding: 0 14px;
    font: var(--type-footnote);
    font-weight: 500;
    color: var(--label);
    cursor: pointer;
    white-space: nowrap;
    transition: opacity var(--duration-fast);
  }
  .segment.selected {
    font-weight: 600;
  }
  .segment:active:not(.selected) {
    opacity: 0.5;
  }
  /* Hairline dividers between unselected segments */
  .segment::before {
    content: '';
    position: absolute;
    left: 0;
    top: 25%;
    bottom: 25%;
    width: 1px;
    background: var(--separator);
    transition: opacity var(--duration-fast);
  }
  .segment.hide-divider::before {
    opacity: 0;
  }
</style>
