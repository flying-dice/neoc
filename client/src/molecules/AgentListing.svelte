<!-- @molecule AgentListing — store-style listing: icon, name, blurb, GET / OPEN / UPDATE pill (or none), optional Remove.
  Atoms: IconTile, Icon, Text, Button, ActivityIndicator.
  Test ids: `testid` on the listing, `${testid}-get` on the main button, `${testid}-remove` on Remove. -->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { ActivityIndicator, IconTile, Icon, Text, Button } from '@agentos/ui/atoms'

  interface Props {
    testid: string
    name: string
    blurb: string
    icon: IconName
    background: string
    /** Shorthand for action="open" vs "get". */
    installed?: boolean
    /** `none` shows the listing without a button (nothing to do with it). */
    action?: 'get' | 'open' | 'update' | 'none'
    busy?: boolean
    onget?: () => void
    /** Shows a Remove button (installed apps). */
    onremove?: () => void
  }

  let {
    testid,
    name,
    blurb,
    icon,
    background,
    installed = false,
    action,
    busy = false,
    onget,
    onremove
  }: Props = $props()

  const kind = $derived(action ?? (installed ? 'open' : 'get'))
  const label = $derived(
    kind === 'none' ? '' : ({ get: 'GET', open: 'OPEN', update: 'UPDATE' } as const)[kind]
  )
</script>

<article class="listing" data-testid={testid} aria-busy={busy}>
  <IconTile size={72} {background} shadow>
    <Icon name={icon} size={38} weight={1.8} color="#fff" />
  </IconTile>
  <div class="meta">
    <Text style="subheadline" weight="semibold" truncate>{name}</Text>
    <Text style="caption1" color="secondary">{blurb}</Text>
    <span class="cta">
      {#if busy}
        <ActivityIndicator label="Working on {name}" />
      {:else if kind !== 'none'}
        <Button
          testid="{testid}-get"
          variant="gray"
          size="small"
          capsule
          aria-label="{label} {name}"
          onclick={onget}
        >
          {label}
        </Button>
        {#if onremove}
          <Button
            testid="{testid}-remove"
            variant="plain"
            size="small"
            destructive
            aria-label="Remove {name}"
            onclick={onremove}
          >
            Remove
          </Button>
        {/if}
      {/if}
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
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 6px;
    min-height: 24px;
  }
  .cta :global(.ios-button) {
    font-size: 12px;
    font-weight: 700;
    min-height: 24px;
    padding: 0 16px;
  }
</style>
