<!--
  @organism Dock — iPadOS floating dock of app icons: the apps the person keeps there (`apps`), then, after a divider,
  the recent apps that are open but not kept (`recents`), as iPadOS shows them. With nothing in either the Dock is not drawn.
  Molecules: AppIcon. Atoms: Material.
  Test ids: `testid` on the tray, `${testid}-${app.id}` on each app (of either section), `${testid}-${app.id}-name`
  on the name capsule, `${testid}-divider` between the sections (there only when both have apps).

  `onmenu` gives every icon a context menu (see AppIcon): it is called with the app's id and the icon's button.

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
    /** The apps kept in the Dock, in the person's order. */
    apps: DockApp[]
    /** Open apps that are not kept in the Dock, most recent first: drawn after a divider. */
    recents?: DockApp[]
    /** The width the screen leaves the Dock, in px (the Dock keeps one row inside it); unset, the Dock is as wide as its icons. */
    available?: number
    onselect?: (id: string) => void
    /** Ask for an icon's context menu. */
    onmenu?: (id: string, anchor: HTMLElement) => void
  }

  let { testid, apps, recents = [], available, onselect, onmenu }: Props = $props()

  const ICON = 58
  const GAP = 18
  const TIGHT_GAP = 8
  const MIN_ICON = 44 // the smallest target the design language allows
  const PADDING = 16 // the tray's inset at each side
  const DIVIDER = 1 // the line between the kept apps and the recent ones

  const divided = $derived(apps.length > 0 && recents.length > 0)

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
    if (available === undefined || count === 0) return { size: ICON, gap: GAP, scrolls: false }
    const room = available - 2 * PADDING
    // the divider is one more thing in the row: its own width and one more gap
    const line = (gap: number): number => (divided ? DIVIDER + gap : 0)
    const width = (size: number, gap: number): number =>
      count * size + (count - 1) * gap + line(gap)
    if (width(ICON, GAP) <= room) return { size: ICON, gap: GAP, scrolls: false }
    if (width(ICON, TIGHT_GAP) <= room) return { size: ICON, gap: TIGHT_GAP, scrolls: false }
    const size = Math.max(
      MIN_ICON,
      Math.min(ICON, Math.floor((room - (count - 1) * TIGHT_GAP - line(TIGHT_GAP)) / count))
    )
    return { size, gap: TIGHT_GAP, scrolls: width(size, TIGHT_GAP) > room }
  })
</script>

{#snippet icon(app: DockApp)}
  <span class="item">
    <AppIcon
      testid="{testid}-{app.id}"
      name={app.name}
      icon={app.icon}
      background={app.background}
      running={app.running}
      size={fit.size}
      onclick={() => onselect?.(app.id)}
      onmenu={onmenu ? (anchor) => onmenu(app.id, anchor) : undefined}
    />
    <span class="name" aria-hidden="true" data-testid="{testid}-{app.id}-name">{app.name}</span>
  </span>
{/snippet}

{#if apps.length + recents.length > 0}
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
        {@render icon(app)}
      {/each}
      {#if divided}
        <span
          class="divider"
          role="separator"
          aria-orientation="vertical"
          aria-label="Recent apps"
          style:height="{Math.round(fit.size * 0.72)}px"
          data-testid="{testid}-divider"
        ></span>
      {/if}
      {#each recents as app (app.id)}
        {@render icon(app)}
      {/each}
    </div>
  </Material>
{/if}

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
  /* the line between the kept apps and the recent ones: a separator that holds 3:1 on the tray in both themes */
  .divider {
    flex: none;
    width: 1px;
    border-radius: 1px;
    background: var(--label-secondary);
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
</style>
