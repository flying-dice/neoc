<!-- @molecule TabBarItem — a single UITabBarItem. Atoms: Icon, Badge. Test id: `testid`. -->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import Badge from '../atoms/Badge.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    label: string
    icon: IconName
    active?: boolean
    badge?: number
    onclick?: () => void
  }

  let { testid, label, icon, active = false, badge, onclick }: Props = $props()
</script>

<button
  type="button"
  class="ios-tab"
  data-testid={testid}
  class:active
  aria-current={active ? 'page' : undefined}
  {onclick}
>
  <span class="glyph">
    <Icon name={icon} size={24} weight={active ? 2.3 : 1.9} />
    {#if badge}<span class="badge"><Badge value={badge} /></span>{/if}
  </span>
  <span class="label">{label}</span>
</button>

<style>
  .ios-tab {
    appearance: none;
    border: 0;
    background: none;
    flex: 1;
    max-width: 120px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 2px 0;
    color: var(--ios-gray);
    cursor: pointer;
  }
  .active {
    color: var(--tint);
  }
  .glyph {
    position: relative;
    display: flex;
  }
  .badge {
    position: absolute;
    top: -4px;
    left: 15px;
  }
  .label {
    font: 500 10px/12px var(--font-text);
    letter-spacing: 0.1px;
  }
</style>
