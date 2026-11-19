<!--
  @organism Dock — iPadOS floating dock with pinned apps and a recents group.
  Molecules: AppIcon. Atoms: Material.
  Test ids: `testid` on the tray, `${testid}-${app.id}` on each app (pinned and recents), `${testid}-${app.id}-name`
  on the name capsule.

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
    onselect?: (id: string) => void
  }

  let { testid, apps, recents = [], iconSize = 58, onselect }: Props = $props()
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
      size={iconSize}
      showLabel={false}
      onclick={() => onselect?.(app.id)}
    />
    <span class="name" aria-hidden="true" data-testid="{testid}-{app.id}-name">{app.name}</span>
  </span>
{/snippet}

<Material thickness="thin" radius="var(--radius-dock)" class="ios-dock">
  <div class="tray" data-testid={testid} data-launcher>
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
  .item {
    position: relative;
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
