<!--
  @organism Dock — iPadOS floating dock with pinned apps and a recents group.
  Molecules: AppIcon. Atoms: Material.
  Test ids: `testid` on the tray, `${testid}-${app.id}` on each app (pinned and recents).
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

<Material thickness="thin" radius="var(--radius-dock)" class="ios-dock">
  <div class="tray" data-testid={testid}>
    {#each apps as app (app.id)}
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
    {/each}
    {#if recents.length}
      <span class="divider" aria-hidden="true"></span>
      {#each recents as app (app.id)}
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
  .divider {
    align-self: stretch;
    width: 1px;
    margin: 4px 0;
    background: var(--separator);
  }
</style>
