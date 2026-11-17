<!-- @molecule CapabilityCard — titled card listing data/permission chips. Atoms: Icon, Text. -->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { Icon, Text } from '@agentos/ui/atoms'

  interface Props {
    title: string
    description: string
    icon: IconName
    items: { icon: IconName; label: string }[]
  }

  let { title, description, icon, items }: Props = $props()
</script>

<article class="card">
  <span class="glyph"><Icon name={icon} size={30} weight={1.8} /></span>
  <Text style="subheadline" weight="semibold">{title}</Text>
  <Text style="caption1" color="secondary">{description}</Text>
  <ul>
    {#each items as item (item.label)}
      <li>
        <Icon name={item.icon} size={14} weight={2.2} />
        <Text style="caption1">{item.label}</Text>
      </li>
    {/each}
  </ul>
</article>

<style>
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 20px 18px;
    border-radius: 12px;
    background: var(--bg-secondary);
    text-align: center;
  }
  .glyph {
    color: var(--tint);
    margin-bottom: 4px;
  }
  ul {
    display: grid;
    grid-template-columns: repeat(2, auto);
    gap: 8px 16px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
