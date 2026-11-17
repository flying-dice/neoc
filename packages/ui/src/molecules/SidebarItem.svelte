<!-- @molecule SidebarItem — macOS source-list row (tinted glyph + label). Atoms: Icon, Badge. Test id: `testid`. -->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import Badge from '../atoms/Badge.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    label: string
    icon: IconName
    selected?: boolean
    badge?: number
    onclick?: () => void
  }

  let { testid, label, icon, selected = false, badge, onclick }: Props = $props()
</script>

<button
  type="button"
  class="sidebar-item"
  data-testid={testid}
  class:selected
  aria-current={selected ? 'page' : undefined}
  {onclick}
>
  <span class="glyph"><Icon name={icon} size={16} weight={2} /></span>
  <span class="label">{label}</span>
  {#if badge}<Badge value={badge} color="var(--ios-gray)" />{/if}
</button>

<style>
  .sidebar-item {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    border-radius: 6px;
    font: 400 14px/18px var(--font-text);
    color: var(--label);
    text-align: left;
    cursor: default;
  }
  .glyph {
    display: flex;
    color: var(--tint);
  }
  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sidebar-item:hover {
    background: var(--fill-quaternary);
  }
  .selected,
  .selected:hover {
    background: var(--fill-secondary);
  }
</style>
