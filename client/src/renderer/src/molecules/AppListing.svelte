<!--
  @molecule AppListing — an installable Luau app in Discover: icon, name, version and where it comes from, its
  state, and what you can do (GET / OPEN / ENABLE, Disable while it runs, Remove). Atoms: IconTile, Icon, Text, Button,
  ActivityIndicator.
  Test ids: `testid` on the listing, `${testid}-get` (the main button), `${testid}-toggle` (Disable, only while the app runs),
  `${testid}-remove`, `${testid}-state`.
-->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { ActivityIndicator, Button, Icon, IconTile, Text } from '@agentos/ui/atoms'
  import type { AppState } from '../shell/hostApi'

  interface Props {
    testid: string
    name: string
    /** `0.1.0 · this computer` */
    detail: string
    description?: string
    icon: IconName
    background: string
    /** Not installed: GET. Installed: OPEN (running), ENABLE (disabled) or nothing to open (failed). */
    state?: AppState
    /** Why it failed. */
    message?: string
    busy?: boolean
    onget: () => void
    ontoggle?: () => void
    onremove?: () => void
  }

  let {
    testid,
    name,
    detail,
    description,
    icon,
    background,
    state,
    message,
    busy = false,
    onget,
    ontoggle,
    onremove
  }: Props = $props()

  const label = $derived(
    state === undefined
      ? 'GET'
      : state === 'running'
        ? 'OPEN'
        : state === 'disabled'
          ? 'ENABLE'
          : 'RETRY'
  )
</script>

<article class="listing" data-testid={testid} aria-busy={busy}>
  <IconTile size={72} {background} shadow>
    <Icon name={icon} size={38} weight={1.8} color="#fff" />
  </IconTile>
  <div class="meta">
    <Text style="subheadline" weight="semibold" truncate>{name}</Text>
    <Text style="caption1" color="secondary">{detail}</Text>
    {#if description}<Text style="caption1" color="secondary">{description}</Text>{/if}
    {#if state === 'disabled'}
      <span data-testid="{testid}-state"
        ><Text style="caption1" color="secondary">Disabled: agents cannot use it</Text></span
      >
    {:else if state === 'failed'}
      <span data-testid="{testid}-state"
        ><Text style="caption1" color="destructive"
          >Failed to start{message ? `: ${message}` : ''}</Text
        ></span
      >
    {/if}
    <span class="cta">
      {#if busy}
        <ActivityIndicator label="Working on {name}" />
      {:else}
        <Button
          testid="{testid}-get"
          variant="gray"
          size="small"
          capsule
          aria-label="{label.toLowerCase()} {name}"
          onclick={onget}>{label}</Button
        >
        {#if ontoggle && state === 'running'}
          <Button
            testid="{testid}-toggle"
            variant="plain"
            size="small"
            aria-label="Disable {name}"
            onclick={ontoggle}>Disable</Button
          >
        {/if}
        {#if onremove && state}
          <Button
            testid="{testid}-remove"
            variant="plain"
            size="small"
            destructive
            aria-label="Remove {name}"
            onclick={onremove}>Remove</Button
          >
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
    flex-wrap: wrap;
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
