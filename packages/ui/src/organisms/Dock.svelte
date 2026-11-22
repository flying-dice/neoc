<!--
  @organism Dock — iPadOS floating dock with pinned apps and a recents group.
  Molecules: AppIcon. Atoms: Material.
  Test ids: `testid` on the tray, `${testid}-${app.id}` on each app (pinned and recents), `${testid}-${app.id}-name`
  on the name capsule.

  Width: with `available` (the room the screen leaves it) the Dock stays one row, like iPadOS: when the icons do not fit it
  tightens the gaps, then shrinks the icons down to the 44 px the design language allows for a target, and only then
  scrolls sideways inside itself (every icon stays reachable by pointer and by keyboard: focus scrolls it fully into view).

  The tray is a `data-launcher`: focus on one of its icons yields to the window it opens (see Window, Focus).

  Names: an icon is only a picture, so on hover and keyboard focus (`:focus-visible`, not a click) a small flat capsule
  above it says which app it is. The capsule is `aria-hidden`: the button's own accessible name already says it.
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import AppIcon from '../molecules/AppIcon.svelte'
  import type { DockApp } from '../types'

  interface Props {
    testid: string
    apps: DockApp[]
    /** Shown after a divider, like iPadOS suggested & recent apps. */
    recents?: DockApp[]
    iconSize?: number
    /** The width the screen leaves the Dock, in px (the Dock keeps one row inside it); unset, the Dock is as wide as its icons. */
    available?: number
    onselect?: (id: string) => void
  }

  let { testid, apps, recents = [], iconSize = 58, available, onselect }: Props = $props()

  const GAP = 18
  const TIGHT_GAP = 8
  const MIN_ICON = 44 // the smallest target the design language allows
  const PADDING = 16 // the tray's inset at each side

  /**
   * Focus brings an icon into view by the browser's own rule, which can leave it half hidden at an edge of a tray that scrolls:
   * the focused icon is scrolled all the way in, with the tray's inset beside it.
   */
  function keepFocusedIconInView(event: FocusEvent): void {
    const tray = event.currentTarget as HTMLElement
    const icon = (event.target as HTMLElement).closest<HTMLElement>('.item')
    if (!icon || tray.scrollWidth <= tray.clientWidth) return
    const t = tray.getBoundingClientRect()
    const i = icon.getBoundingClientRect()
    if (i.left < t.left + PADDING) tray.scrollLeft -= t.left + PADDING - i.left
    else if (i.right > t.right - PADDING) tray.scrollLeft += i.right - (t.right - PADDING)
  }

  /** The icon size and gap that make the tray fit `available`, and whether it still has to scroll at the smallest. */
  const fit = $derived.by(() => {
    const count = apps.length + recents.length
    if (available === undefined || count === 0) return { size: iconSize, gap: GAP, scrolls: false }
    const dividers = recents.length > 0 ? 1 : 0
    const room = available - 2 * PADDING
    const width = (size: number, gap: number): number =>
      count * size + dividers + (count + dividers - 1) * gap
    if (width(iconSize, GAP) <= room) return { size: iconSize, gap: GAP, scrolls: false }
    if (width(iconSize, TIGHT_GAP) <= room) return { size: iconSize, gap: TIGHT_GAP, scrolls: false }
    const size = Math.max(
      MIN_ICON,
      Math.min(iconSize, Math.floor((room - dividers - (count + dividers - 1) * TIGHT_GAP) / count))
    )
    return { size, gap: TIGHT_GAP, scrolls: width(size, TIGHT_GAP) > room }
  })
</script>

{#snippet dockItem(app: DockApp)}
  <span class="item">
    <AppIcon
      testid="{testid}-{app.id}"
      name={app.name}
      icon={app.icon}
      background={app.background}
      badge={app.badge}
      running={app.running}
      size={fit.size}
      showLabel={false}
      onclick={() => onselect?.(app.id)}
    />
    <span class="name" aria-hidden="true" data-testid="{testid}-{app.id}-name">{app.name}</span>
  </span>
{/snippet}

<Material thickness="thin" radius="var(--radius-dock)" class="ios-dock">
  <div
    class="tray"
    class:scrolls={fit.scrolls}
    style:gap="{fit.gap}px"
    style:max-width={fit.scrolls && available !== undefined ? `${available}px` : undefined}
    data-scrolls-x={fit.scrolls ? '' : undefined}
    onfocusin={keepFocusedIconInView}
    data-testid={testid}
    data-launcher
  >
    {#each apps as app (app.id)}
      {@render dockItem(app)}
    {/each}
    {#if recents.length}
      <span class="divider" aria-hidden="true"></span>
      {#each recents as app (app.id)}
        {@render dockItem(app)}
      {/each}
    {/if}
  </div>
</Material>

<style>
  .tray {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 14px 16px;
    border-radius: var(--radius-dock);
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 0.25),
      0 10px 30px rgb(0 0 0 / 0.18);
  }
  /* only when even the smallest icons do not fit: the icons are reached by scrolling the tray (their names are clipped then) */
  .tray.scrolls {
    box-sizing: border-box;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .item {
    position: relative;
    flex: none;
    display: inline-flex;
  }
  /* A flat capsule (no shadow, no material) in the neutral pair that holds 4.5:1 on any accent or wallpaper. */
  .name {
    position: absolute;
    bottom: calc(100% + 10px);
    left: 50%;
    transform: translateX(-50%);
    padding: 4px 12px;
    border-radius: var(--radius-pill);
    background: var(--label);
    color: var(--bg-elevated);
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--duration-fast) var(--ease-ios);
    z-index: 1;
  }
  .item:hover .name,
  .item:has(:global(:focus-visible)) .name {
    opacity: 1;
  }
  .divider {
    align-self: stretch;
    width: 1px;
    margin: 4px 0;
    background: var(--separator);
  }
</style>
