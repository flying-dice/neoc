<!--
  @template DevicesPane — the paired devices of this host (Settings > Devices): which browsers may use it, when
  each was last seen, and Revoke for the ones that should not any more. The device asking is marked and has no
  Revoke (revoking yourself would only send you back to the pairing screen). Atoms: Button, Text. Molecules:
  ListRow, Alert.
  Test ids: `settings-devices` (the list), `settings-device-<id>` (a row), `settings-device-<id>-revoke`,
  `settings-device-revoke-alert` (+ `-action-revoke`, `-action-cancel`), `settings-devices-error`.
-->
<script lang="ts">
  import { Button, Text } from '@agentos/ui/atoms'
  import { Alert, ListRow } from '@agentos/ui/molecules'
  import { ListSection } from '@agentos/ui/organisms'
  import { host } from '../lib/host.svelte'
  import { ago, type DeviceRow } from '../shell/hostApi'

  let devices = $state<DeviceRow[]>([])
  let error = $state<string | null>(null)
  let loaded = $state(false)
  let revoking = $state<DeviceRow | null>(null)
  let confirming = $state(false)

  const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

  async function load(): Promise<void> {
    try {
      devices = (await host.api.call<{ devices: DeviceRow[] }>('host.devices.list')).devices
      error = null
    } catch (e) {
      error = messageOf(e)
    } finally {
      loaded = true
    }
  }

  async function revoke(device: DeviceRow): Promise<void> {
    try {
      await host.api.call('host.devices.revoke', { id: device.id })
    } catch (e) {
      error = messageOf(e)
    }
    await load()
  }

  $effect(() => {
    void load()
  })

  const seen = (d: DeviceRow): string =>
    d.current ? 'Using now' : `Last seen ${ago(d.lastSeenAt, Date.now())}`
</script>

{#if error}
  <p class="error" role="alert" data-testid="settings-devices-error">{error}</p>
{/if}

<div data-testid="settings-devices">
  <ListSection
    header="Paired devices"
    footer="A device that is paired can use this host and everything on it. Revoke one you no longer trust; it must pair again with a new code (run “agentos-host pair”)."
  >
    {#each devices as d (d.id)}
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
              onclick={() => {
                revoking = d
                confirming = true
              }}>Revoke</Button
            >
          {/if}
        {/snippet}
      </ListRow>
    {:else}
      {#if loaded}
        <ListRow testid="settings-devices-empty" title="No paired devices" />
      {/if}
    {/each}
  </ListSection>
</div>

{#if revoking}
  <Alert
    testid="settings-device-revoke-alert"
    bind:open={confirming}
    title="Revoke {revoking.name}?"
    message="It will be signed out of AgentOS and must pair again to come back."
    actions={[
      { label: 'Cancel', style: 'cancel' },
      {
        label: 'Revoke',
        style: 'destructive',
        onclick: () => {
          const d = revoking
          if (d) void revoke(d)
        }
      }
    ]}
  />
{/if}

<style>
  .error {
    margin: 0 0 8px;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
