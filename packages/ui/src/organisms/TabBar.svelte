<!--
  @organism TabBar — UITabBar. Molecules: TabBarItem. Atoms: Material.
  Test ids: `testid` on the bar, `${testid}-${tab.value}` on each tab.
-->
<script lang="ts" generics="T extends string">
  import Material from '../atoms/Material.svelte'
  import TabBarItem from '../molecules/TabBarItem.svelte'
  import type { IconName } from '../icons'

  interface Tab {
    value: T
    label: string
    icon: IconName
    badge?: number
  }

  interface Props {
    testid: string
    tabs: Tab[]
    value?: T
    onchange?: (value: T) => void
  }

  let { testid, tabs, value = $bindable(), onchange }: Props = $props()

  function select(v: T): void {
    value = v
    onchange?.(v)
  }
</script>

<Material thickness="chrome" as="nav" class="ios-tabbar">
  <div class="items" data-testid={testid}>
    {#each tabs as tab (tab.value)}
      <TabBarItem
        testid="{testid}-{tab.value}"
        label={tab.label}
        icon={tab.icon}
        badge={tab.badge}
        active={tab.value === value}
        onclick={() => select(tab.value)}
      />
    {/each}
  </div>
</Material>

<style>
  .items {
    display: flex;
    justify-content: center;
    gap: 4px;
    padding: 6px 8px 8px;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
</style>
