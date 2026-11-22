<!--
  @template DevicesPane — the paired devices of this host (Settings > Devices): which browsers may use it, when
  each was last seen, and Revoke for the ones that should not any more. The device asking is marked and has no
  Revoke (revoking yourself would only send you back to the pairing screen). Revoke only asks: the page that hosts
  this pane shows the confirmation (`devices.asked`). A problem (the list could not be read, a device could not be
  revoked) is said above the list, with Try Again; the list under it is the last one the host gave, and before the
  first one there is no list at all, rather than an empty one that would say nobody is paired.
  Atoms: Button, Text. Molecules: ListRow.
  Test ids: `settings-devices` (the list, which takes focus when a revoked row had it and no other is left),
  `settings-device-<id>` (a row), `settings-device-<id>-revoke`, `settings-devices-empty`, `settings-devices-error`,
  `settings-devices-retry`.
-->
<script lang="ts">
  import { Button, Text } from '@vos/ui/atoms'
  import { ListRow } from '@vos/ui/molecules'
  import { ListSection } from '@vos/ui/organisms'
  import { devices } from '../lib/devices.svelte'
  import { ago, type DeviceRow } from '../shell/hostApi'

  $effect(() => {
    void devices.load()
  })

  const seen = (d: DeviceRow): string =>
    d.current ? 'Using now' : `Last seen ${ago(d.lastSeenAt, Date.now())}`
</script>

{#if devices.error}
  <div class="error" role="alert" data-testid="settings-devices-error">
    <p>{devices.error}</p>
    <Button
      testid="settings-devices-retry"
      variant="plain"
      size="small"
      onclick={() => devices.load()}>Try Again</Button
    >
  </div>
{/if}

{#if devices.loaded}
  <div
    class="list"
    data-testid="settings-devices"
    role="group"
    aria-label="Paired devices"
    tabindex="-1"
  >
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
        <ListRow testid="settings-devices-empty" title="No paired devices" />
      {/each}
    </ListSection>
  </div>
{/if}

<style>
  .error {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 12px;
    margin: 0 0 16px;
    /* in line with the text of the rows under it */
    padding: 0 calc(var(--list-inset, 20px) + var(--row-inset));
  }
  .error p {
    margin: 0;
    min-width: 0;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    color: var(--ios-red);
    overflow-wrap: anywhere;
  }
</style>
