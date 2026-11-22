<!--
  @molecule ContextMenu — the iPadOS context menu: a short list of actions on the thing that was long-pressed,
  right-clicked or asked for from the keyboard. It fills the positioned box it is put in (a layer over the screen) and
  places itself beside `anchor`, a box in viewport coordinates: below it, or above when there is no room below, and
  always wholly inside the layer (see `placeMenu`).
  Atoms: Material, Icon.
  Test ids: `testid` on the menu, `${testid}-layer` on the layer, `${testid}-<item id>` on each item.

  Keyboard (the menu pattern): the menu takes focus on its first item when it opens; Up and Down move without wrapping,
  Home and End go to the ends; one item is in the tab order at a time. Enter and Space choose. Escape and Tab call
  `ondismiss`, and so does a press anywhere outside the menu (which is not passed on to what is under it, as on
  iPadOS: the first tap only closes the menu). The owner removes the menu and decides where focus goes: back to the
  thing it was opened on, usually. Escape does not reach anything behind the menu.
-->
<script lang="ts">
  import { scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import { moveIndex } from '../keys'
  import { motion } from '../motion'
  import { placeMenu, type Placement } from '../placement'
  import type { AnchorRect, MenuItem } from '../types'

  interface Props {
    testid: string
    /** What the menu is about, for assistive technology ("Calculator"). */
    label: string
    items: MenuItem[]
    /** The box the menu opens beside, in viewport coordinates. */
    anchor: AnchorRect
    onselect?: (id: string) => void
    /** The person left the menu without choosing. */
    ondismiss?: (how: 'escape' | 'tab' | 'outside') => void
  }

  let { testid, label, items, anchor, onselect, ondismiss }: Props = $props()

  let layer: HTMLElement | undefined = $state()
  let menu: HTMLElement | undefined = $state()
  /** The item that holds the tab stop (the one last focused; the first until then). */
  let tabStop = $state(0)
  let placed = $state<Placement | null>(null)

  const buttons = (): HTMLElement[] => [
    ...(menu?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])
  ]

  // Placed once it can be measured (an effect runs before the first paint, so it is never seen at the layer's corner),
  // and again when the anchor or the screen changes.
  $effect(() => {
    if (!layer || !menu) return
    const place = (): void => {
      if (!layer || !menu) return
      const bounds = layer.getBoundingClientRect()
      placed = placeMenu(
        { ...anchor, left: anchor.left - bounds.left, top: anchor.top - bounds.top },
        { width: menu.offsetWidth, height: menu.offsetHeight },
        { width: bounds.width, height: bounds.height }
      )
    }
    place()
    const observer = new ResizeObserver(place)
    observer.observe(layer)
    return () => observer.disconnect()
  })

  // A menu that opens takes the focus, on its first item.
  $effect(() => {
    if (menu) buttons()[0]?.focus({ preventScroll: true })
  })

  function onfocusin(e: FocusEvent): void {
    const at = buttons().indexOf(e.target as HTMLElement)
    if (at >= 0) tabStop = at
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    const all = buttons()
    const at = all.indexOf(document.activeElement as HTMLElement)
    const next = moveIndex(all.length, at < 0 ? tabStop : at, e.key, 'vertical', false) // the ends do not wrap
    if (next !== null) all[next]?.focus({ preventScroll: true })
    else if (e.key === 'Escape' || e.key === 'Tab') ondismiss?.(e.key === 'Tab' ? 'tab' : 'escape')
    else return
    e.preventDefault()
    e.stopPropagation() // the menu's own keys reach nothing behind it
  }

  /** A press on the layer itself (not on the menu) leaves the menu, and goes no further. */
  function onlayerpointerdown(e: PointerEvent): void {
    if (menu?.contains(e.target as Node)) return
    e.preventDefault()
    e.stopPropagation()
    ondismiss?.('outside')
  }
</script>

<div
  class="layer"
  role="presentation"
  data-testid="{testid}-layer"
  bind:this={layer}
  onpointerdown={onlayerpointerdown}
  oncontextmenu={(e) => e.preventDefault()}
>
  <!-- svelte-ignore a11y_interactive_supports_focus (the items are the focusable parts of a menu) -->
  <div
    class="menu"
    class:above={placed?.side === 'above'}
    role="menu"
    tabindex="-1"
    aria-label={label}
    data-testid={testid}
    bind:this={menu}
    style:left="{placed?.left ?? 0}px"
    style:top="{placed?.top ?? 0}px"
    {onkeydown}
    {onfocusin}
    in:scale={{ start: 0.92, duration: motion(160), easing: cubicOut }}
  >
    <Material thickness="thick" radius="var(--radius-lg)">
      {#each items as item, i (item.id)}
        <button
          type="button"
          role="menuitem"
          class="item"
          class:destructive={item.destructive}
          tabindex={tabStop === i ? 0 : -1}
          data-testid="{testid}-{item.id}"
          onclick={() => onselect?.(item.id)}
        >
          <span class="label">{item.label}</span>
          {#if item.icon}<Icon name={item.icon} size={19} weight={2} />{/if}
        </button>
      {/each}
    </Material>
  </div>
</div>

<style>
  .layer {
    position: absolute;
    inset: 0;
    /* the first press outside only closes the menu: nothing under the layer is pressed */
    pointer-events: auto;
    touch-action: none;
  }
  .menu {
    position: absolute;
    width: max-content;
    min-width: 220px;
    max-width: calc(100% - 16px);
    max-height: calc(100% - 16px);
    overflow: auto;
    border-radius: var(--radius-lg);
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.1),
      var(--shadow-popover);
    color: var(--label);
    transform-origin: top center;
  }
  .menu.above {
    transform-origin: bottom center;
  }
  .item {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    min-height: 44px;
    padding: 0 16px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    text-align: start;
    cursor: pointer;
  }
  .item + .item {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .item:hover,
  .item:active {
    background: var(--fill-quaternary);
  }
  .item:focus-visible {
    outline-offset: -3px;
    border-radius: var(--radius-md);
  }
  .label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .destructive {
    color: var(--ios-red);
  }
</style>
