<!--
  @organism MenuBar — macOS-style translucent menu bar with pull-down menus.
  Click a title to open; while open, hovering another title switches menus.
  Molecules: Menu, MenuBarExtras. Atoms: Icon.
  Test ids: `testid` on the bar, `${testid}-<title slug>` on each menu title,
  `${testid}-<title slug>-menu` on its open menu (items `…-menu-<label slug>`),
  `${testid}-extras` (MenuBarExtras).
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import Menu from '../molecules/Menu.svelte'
  import MenuBarExtras from '../molecules/MenuBarExtras.svelte'
  import type { MenuDefinition } from '../types'

  interface Props {
    testid: string
    menus: MenuDefinition[]
    battery?: number
    charging?: boolean
    wifi?: boolean
    onsearch?: () => void
    oncontrolcenter?: () => void
  }

  let { testid, menus, battery = 1, charging = false, wifi = true, onsearch, oncontrolcenter }: Props =
    $props()

  let openIndex = $state<number | null>(null)
  let bar: HTMLElement | undefined = $state()

  const slug = (label: string): string =>
    label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  function toggle(i: number): void {
    openIndex = openIndex === i ? null : i
  }

  function onwindowpointerdown(e: PointerEvent): void {
    if (openIndex !== null && bar && !bar.contains(e.target as Node)) openIndex = null
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') openIndex = null
  }
</script>

<svelte:window onpointerdown={onwindowpointerdown} {onkeydown} />

<nav class="menubar" bind:this={bar} aria-label="Menu bar" data-testid={testid}>
  <div class="menus" role="menubar">
    {#each menus as menu, i (menu.title)}
      <div class="slot">
        <button
          type="button"
          class="title"
          data-testid="{testid}-{slug(menu.title)}"
          class:bold={menu.bold}
          class:open={openIndex === i}
          aria-haspopup="menu"
          aria-expanded={openIndex === i}
          aria-label={menu.icon ? menu.title : undefined}
          onclick={() => toggle(i)}
          onpointerenter={() => openIndex !== null && (openIndex = i)}
        >
          {#if menu.icon}<Icon name={menu.icon} size={15} weight={2.2} />{:else}{menu.title}{/if}
        </button>
        {#if openIndex === i}
          <div class="dropdown">
            <Menu testid="{testid}-{slug(menu.title)}-menu" items={menu.items} label={menu.title} onselect={() => (openIndex = null)} />
          </div>
        {/if}
      </div>
    {/each}
  </div>
  <MenuBarExtras testid="{testid}-extras" {battery} {charging} {wifi} {onsearch} {oncontrolcenter} />
</nav>

<style>
  .menubar {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 26px;
    padding: 0 8px;
    color: #fff;
    background: rgb(20 20 40 / 0.22);
    -webkit-backdrop-filter: blur(40px) saturate(180%);
    backdrop-filter: blur(40px) saturate(180%);
    text-shadow: 0 0 6px rgb(0 0 0 / 0.12);
  }
  .menus {
    display: flex;
    align-items: center;
  }
  .slot {
    position: relative;
  }
  .title {
    appearance: none;
    border: 0;
    background: transparent;
    color: inherit;
    display: flex;
    align-items: center;
    height: 22px;
    padding: 0 9px;
    border-radius: 4px;
    font: 400 13px/1 var(--font-text);
    white-space: nowrap;
    cursor: default;
  }
  .bold {
    font-weight: 700;
  }
  .title.open,
  .title:active {
    background: rgb(255 255 255 / 0.22);
  }
  .dropdown {
    position: absolute;
    top: calc(100% + 3px);
    left: 0;
    z-index: 1000;
    text-shadow: none;
  }
</style>
