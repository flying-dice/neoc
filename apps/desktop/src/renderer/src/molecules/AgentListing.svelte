<!-- @molecule AgentListing — store-style listing: icon, name, blurb, GET pill. Atoms: IconTile, Icon, Text, Button.
  Test ids: `testid` on the listing, `${testid}-get` on the GET / OPEN button. -->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { IconTile, Icon, Text, Button } from '@agentos/ui/atoms'

  interface Props {
    testid: string
    name: string
    blurb: string
    icon: IconName
    background: string
    installed?: boolean
    onget?: () => void
  }

  let { testid, name, blurb, icon, background, installed = false, onget }: Props = $props()
</script>

<article class="listing" data-testid={testid}>
  <IconTile size={72} {background} shadow>
    <Icon name={icon} size={38} weight={1.8} color="#fff" />
  </IconTile>
  <div class="meta">
    <Text style="subheadline" weight="semibold" truncate>{name}</Text>
    <Text style="caption1" color="secondary">{blurb}</Text>
    <span class="cta">
      <Button testid="{testid}-get" variant="gray" size="small" capsule onclick={onget}
        >{installed ? 'OPEN' : 'GET'}</Button
      >
    </span>
  </div>
</article>

<style>
  .listing {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    min-width: 0;
  }
  .meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .cta {
    margin-top: 6px;
  }
  .cta :global(.ios-button) {
    font-size: 12px;
    font-weight: 700;
    min-height: 24px;
    padding: 0 16px;
  }
</style>
