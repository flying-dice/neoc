<!--
  @page SettingsPage — the settings of this browser and this host, as an iPadOS Settings split view: General (whether
  notifications show as banners, the host's version), Appearance (theme, accent colour, text size, transparency) and
  Devices (the paired devices, and revoking one). Everything in General and Appearance is the `settings` store, kept in
  this browser; Devices is the host's (`host.devices.*`).

  In a window narrower than the split view allows, the list of panes and the pane are one column at a time: the list
  first, a pane when one is chosen, and Back in the pane's bar. Focus follows (to Back, and back to the pane's row), so
  the keyboard is never left on something that is gone.

  Test ids: `settings` (the page), `settings-pane-<id>` (the list), `settings-nav` (+ `-back`), `settings-banners`,
  `settings-about-host`, `settings-theme` (+ `-light|dark|auto`), `settings-accent-<name>`, `settings-text-size`
  (+ `-value`), `settings-reduce-transparency`, the Devices pane's (see DevicesPane) and
  `settings-device-revoke-alert` (+ `-action-cancel|revoke`).
-->
<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { Toggle, SegmentedControl } from '@vos/ui/atoms'
  import { ListRow, IconSlider, Alert } from '@vos/ui/molecules'
  import { ListSection, NavigationBar } from '@vos/ui/organisms'
  import { SplitViewTemplate, ScreenTemplate } from '@vos/ui/templates'
  import ColorSwatch from '../atoms/ColorSwatch.svelte'
  import DevicesPane from '../templates/DevicesPane.svelte'
  import { settings, accents, accentColor, TEXT_SCALE, type Theme } from '../lib/settings.svelte'
  import { host } from '../lib/host.svelte'
  import { devices } from '../lib/devices.svelte'
  import {
    settingsNavigation,
    settingsPanes as panes,
    type SettingsPane as Pane
  } from '../lib/settingsPanes.svelte'
  import type { DeviceRow } from '../shell/hostApi'

  const themes: { value: Theme; label: string }[] = [
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
    { value: 'auto', label: 'Auto' }
  ]

  let pane = $state<Pane>('general')
  const current = $derived(panes.find((p) => p.id === pane)!)

  /** The window is too narrow for two columns (the split view says so). */
  let collapsed = $state(false)
  /** In a narrow window: the list of panes is on screen, not a pane. A wide window shows both and ignores this. */
  let browsing = $state(true)

  let page: HTMLElement | undefined = $state()
  /** Move focus to a part of this page, once it is drawn. */
  async function focusPart(testid: string): Promise<void> {
    await tick()
    page?.querySelector<HTMLElement>(`[data-testid="${testid}"]`)?.focus()
  }

  function choose(id: Pane): void {
    pane = id
    browsing = false
    // narrow: the row that was chosen is gone with the list, so the keys go to the way back
    if (collapsed) void focusPart('settings-nav-back')
  }

  // The shell asked for a pane (system search opening "Appearance"), before this page was drawn or while it is open:
  // show it. Where the keyboard goes is the asker's business (it has just moved it to this window).
  $effect(() => {
    const asked = settingsNavigation.asked
    if (asked === null) return
    untrack(() => {
      pane = asked
      browsing = false
      settingsNavigation.done()
    })
  })

  function back(): void {
    browsing = true
    void focusPart(`settings-pane-${pane}`)
  }

  // read again when the connection changes: a host that came back may be a newer one
  const hostVersion = $derived((host.connection, host.session?.host?.hostVersion))

  /**
   * Revoke, then put focus where the row was: the next device's Revoke (or the one before), or the list when no other
   * device is left. A device that could not be revoked keeps its row, and the focus the alert gave back to it.
   */
  async function revoke(device: DeviceRow): Promise<void> {
    const revocable = (): DeviceRow[] => devices.list.filter((d) => !d.current)
    const at = revocable().findIndex((d) => d.id === device.id)
    await devices.revoke(device)
    await tick()
    if (page?.contains(document.activeElement)) return
    const left = revocable()
    const next = left[Math.min(Math.max(at, 0), left.length - 1)]
    await focusPart(next ? `settings-device-${next.id}-revoke` : 'settings-devices')
  }
</script>

<div class="settings" data-testid="settings" bind:this={page}>
  <SplitViewTemplate sidebarWidth={300} compact={browsing ? 'sidebar' : 'detail'} bind:collapsed>
    {#snippet sidebar()}
      <NavigationBar testid="settings-sidebar-nav" title="Settings" large />
      <ListSection sidebar>
        {#each panes as p (p.id)}
          <ListRow
            testid="settings-pane-{p.id}"
            title={p.title}
            icon={p.icon}
            iconColor={p.color}
            selected={!collapsed && pane === p.id}
            chevron={collapsed}
            onclick={() => choose(p.id)}
          />
        {/each}
      </ListSection>
    {/snippet}

    {#snippet detail()}
      <ScreenTemplate>
        {#snippet navigationBar()}
          <NavigationBar
            testid="settings-nav"
            title={current.title}
            material
            backLabel={collapsed ? 'Settings' : undefined}
            onback={back}
          />
        {/snippet}

        {#snippet content()}
          <div class="pane">
            {#if pane === 'general'}
              <ListSection
                header="Notifications"
                footer="An app’s notifications appear as banners at the top of the screen. When this is off, they are not shown in this browser, and are not kept for later."
              >
                <ListRow testid="settings-banners-row" title="Show Banners">
                  {#snippet trailing()}
                    <Toggle
                      testid="settings-banners"
                      bind:checked={settings.banners}
                      label="Show Banners"
                    />
                  {/snippet}
                </ListRow>
              </ListSection>
              <ListSection header="About">
                <ListRow
                  testid="settings-about-host"
                  title="Host Version"
                  value={hostVersion ?? 'Not connected'}
                />
              </ListSection>
            {:else if pane === 'appearance'}
              <ListSection header="Theme" footer="Auto follows this device’s light or dark mode.">
                <div class="control">
                  <SegmentedControl
                    testid="settings-theme"
                    label="Theme"
                    bind:value={settings.theme}
                    segments={themes}
                  />
                </div>
              </ListSection>
              <ListSection header="Accent Color">
                <div class="control swatches" role="group" aria-label="Accent color">
                  {#each accents as a (a.id)}
                    <ColorSwatch
                      testid="settings-accent-{a.id}"
                      color={accentColor(a.id)}
                      label={a.name}
                      selected={settings.accent === a.id}
                      onclick={() => (settings.accent = a.id)}
                    />
                  {/each}
                </div>
              </ListSection>
              <ListSection footer="The size of text in vos. Apps choose their own.">
                <ListRow
                  testid="settings-text-size-value"
                  title="Text Size"
                  value="{settings.textScale}%"
                />
                <div class="control">
                  <IconSlider
                    testid="settings-text-size"
                    bind:value={settings.textScale}
                    min={TEXT_SCALE.min}
                    max={TEXT_SCALE.max}
                    step={TEXT_SCALE.step}
                    minIcon="minus"
                    maxIcon="plus"
                    label="Text size, percent"
                  />
                </div>
              </ListSection>
              <ListSection footer="Replaces translucent materials with solid backgrounds.">
                <ListRow testid="settings-reduce-transparency-row" title="Reduce Transparency">
                  {#snippet trailing()}
                    <Toggle
                      testid="settings-reduce-transparency"
                      bind:checked={settings.reduceTransparency}
                      label="Reduce Transparency"
                    />
                  {/snippet}
                </ListRow>
              </ListSection>
            {:else}
              <DevicesPane />
            {/if}
          </div>
        {/snippet}
      </ScreenTemplate>
    {/snippet}
  </SplitViewTemplate>

  {#if devices.asked}
    {@const device = devices.asked}
    <Alert
      testid="settings-device-revoke-alert"
      bind:open={
        () => devices.asked !== null,
        (open) => {
          if (!open) devices.asked = null
        }
      }
      title="Revoke {device.name}?"
      message="It will be signed out of vos and must pair again to come back."
      actions={[
        { label: 'Cancel', style: 'cancel' },
        { label: 'Revoke', style: 'destructive', onclick: () => void revoke(device) }
      ]}
    />
  {/if}
</div>

<style>
  /* the page is its window's content, and what the alert covers */
  .settings {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  .pane {
    max-width: 720px;
    margin: 0 auto;
    padding-top: 16px;
  }
  /* a control that takes the width of its section instead of sitting at the end of a row */
  .control {
    display: grid;
    padding: 10px var(--row-inset);
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    padding-block: 14px;
  }
</style>
