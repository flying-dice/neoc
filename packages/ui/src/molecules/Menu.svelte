<!--
  @molecule Menu — macOS pull-down / context menu panel. Atoms: Material, Icon.
  Test ids: `testid` on the menu, `${testid}-<label slug>` on each item.
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { MenuItem } from '../types'

  interface Props {
    testid: string
    items: MenuItem[]
    label?: string
    /** Called after an item runs, so the owner can close the menu. */
    onselect?: () => void
  }

  let { testid, items, label, onselect }: Props = $props()

  const slug = (label: string): string =>
    label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  function run(item: Extract<MenuItem, { label: string }>): void {
    if (item.disabled) return
    item.onclick?.()
    onselect?.()
  }
</script>

<div class="ios-menu" role="menu" aria-label={label} data-testid={testid}>
  <Material thickness="thick" radius="8px">
    <div class="items">
      {#each items as item, i (i)}
        {#if 'separator' in item}
          <div class="separator" role="separator"></div>
        {:else}
          <button
            type="button"
            role="menuitem"
            data-testid="{testid}-{slug(item.label)}"
            class="item"
            disabled={item.disabled}
            onclick={() => run(item)}
          >
            <span class="check">
              {#if item.checked}<Icon name="checkmark" size={11} weight={3} />{/if}
            </span>
            <span class="label">{item.label}</span>
            {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
          </button>
        {/if}
      {/each}
    </div>
  </Material>
</div>

<style>
  .ios-menu {
    min-width: 220px;
    border-radius: 8px;
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.18),
      0 10px 30px rgb(0 0 0 / 0.25);
    color: var(--label);
  }
  .items {
    display: flex;
    flex-direction: column;
    padding: 5px;
    border-radius: 8px;
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.2);
  }
  .item {
    appearance: none;
    border: 0;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 10px 0 4px;
    border-radius: 4px;
    font: 400 13px/16px var(--font-text);
    color: inherit;
    text-align: left;
    cursor: default;
  }
  .item:hover:not(:disabled),
  .item:focus-visible {
    outline: none;
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  .item:disabled {
    color: var(--label-tertiary);
  }
  .check {
    display: flex;
    width: 14px;
    justify-content: center;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .shortcut {
    margin-left: 24px;
    color: var(--label-secondary);
    letter-spacing: 1px;
  }
  .item:hover:not(:disabled) .shortcut {
    color: rgb(255 255 255 / 0.85);
  }
  .separator {
    height: 1px;
    margin: 5px 10px;
    background: var(--separator);
  }
</style>
