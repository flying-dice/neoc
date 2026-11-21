<!--
  @template DevicesPane — the paired devices of this host (Settings > Devices): which browsers may use it, when
  each was last seen, and Revoke for the ones that should not any more. The device asking is marked and has no
  Revoke (revoking yourself would only send you back to the pairing screen). Revoke only asks: the page that hosts
  this pane shows the confirmation (`devices.asked`). Atoms: Button, Text. Molecules: ListRow.
  Test ids: `settings-devices` (the list), `settings-device-<id>` (a row), `settings-device-<id>-revoke`,
  `settings-devices-error`.
-->
<script lang="ts">
  import { Button, Text } from '@agentos/ui/atoms'
  import { ListRow } from '@agentos/ui/molecules'
  import { ListSection } from '@agentos/ui/organisms'
  import { devices } from '../lib/devices.svelte'
  import { ago, type DeviceRow } from '../shell/hostApi'

  $effect(() => {
    void devices.load()
  })

  const seen = (d: DeviceRow): string =>
    d.current ? 'Using now' : `Last seen ${ago(d.lastSeenAt, Date.now())}`
</script>

{#if devices.error}
  <p class="error" role="alert" data-testid="settings-devices-error">{devices.error}</p>
{/if}

<div data-testid="settings-devices">
  <ListSection
    header="Paired devices"
    footer="A device that is paired can use this host and everything on it. Revoke one you no longer trust; it must pair again with a new code (run “vos pair”)."
  >
    {#each devices.list as d (d.id)}
      <ListRow
        testid="settings-device-{d.id}"
        title={d.name}
        subtitle={seen(d)}
        icon={d.current ? 'checkmark' : 'globe'}
        iconColor={d.current ? 'var(--ios-green)' : 'var(--ios-blue)'}
      >
        {#snippet trailing()}
          {#if d.current}
            <Text style="footnote" color="secondary">This device</Text>
          {:else}
            <Button
              testid="settings-device-{d.id}-revoke"
              variant="plain"
              size="small"
              destructive
              aria-label="Revoke {d.name}"
              onclick={() => (devices.asked = d)}>Revoke</Button
            >
          {/if}
        {/snippet}
      </ListRow>
    {:else}
      {#if devices.loaded}
        <ListRow testid="settings-devices-empty" title="No paired devices" />
      {/if}
    {/each}
  </ListSection>
</div>

<style>
  .error {
    margin: 0 0 8px;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
