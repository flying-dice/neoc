<!--
  @atom SegmentedControl — UISegmentedControl.
  Test ids: `testid` on the group, `${testid}-${segment.value}` on each segment.
-->
<script lang="ts" generics="T extends string">
  import { describe } from '../aria'
  import { moveIndex, nextEnabled } from '../keys'

  interface Segment {
    value: T
    label: string
    disabled?: boolean
  }

  interface Props {
    testid: string
    segments: Segment[]
    value?: T
    block?: boolean
    label?: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    disabled?: boolean
    onchange?: (value: T) => void
  }

  let {
    testid,
    segments,
    value = $bindable(),
    block = false,
    label,
    'aria-description': description,
    disabled = false,
    onchange
  }: Props = $props()

  const index = $derived(Math.max(0, segments.findIndex((s) => s.value === value)))
  const selectedIndex = $derived(segments.findIndex((s) => s.value === value))
  /** The one tab stop: the selected segment, or the first when nothing is selected. */
  const stop = $derived(selectedIndex >= 0 ? selectedIndex : 0)
  let group: HTMLElement | undefined = $state()

  function select(v: T): void {
    if (disabled || v === value) return
    value = v
    onchange?.(v)
  }

  /** Arrow keys move the selection (a radio group: focus and selection travel together). */
  function onkeydown(e: KeyboardEvent): void {
    if (disabled) return
    const from = (e.target as HTMLElement).closest<HTMLElement>('[role="radio"]')
    const current = from ? Number(from.dataset.index) : stop
    const target = moveIndex(segments.length, current, e.key, 'both')
    if (target === null) return
    e.preventDefault()
    const dir = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : -1
    const disabledFlags = segments.map((s) => s.disabled === true)
    const next =
      e.key === 'Home' || e.key === 'End'
        ? target
        : nextEnabled(disabledFlags, current, dir)
    const segment = segments[next]
    if (!segment || segment.disabled) return
    select(segment.value)
    group?.querySelector<HTMLElement>(`[data-index="${next}"]`)?.focus()
  }
</script>

<!-- svelte-ignore a11y_interactive_supports_focus (the radios inside are the tab stops) -->
<div
  class="ios-segmented"
  class:block
  role="radiogroup"
  data-testid={testid}
  aria-label={label}
  {...describe(description)}
  aria-disabled={disabled || undefined}
  bind:this={group}
  {onkeydown}
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
      tabindex={i === stop ? 0 : -1}
      data-index={i}
      disabled={disabled || segment.disabled}
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
  .segment:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .segment:focus-visible {
    outline-offset: -2px;
    border-radius: 7px;
  }
  .segment:active:not(.selected):not(:disabled) {
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
