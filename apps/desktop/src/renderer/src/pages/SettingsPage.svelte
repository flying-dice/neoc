<!-- @page SettingsPage — iPadOS Settings-style system preferences. -->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { hostBridge } from '@agentos/sdk/host/client'
  import { Icon, IconTile, Toggle, SegmentedControl } from '@agentos/ui/atoms'
  import { ListRow, SearchField, IconSlider, Alert } from '@agentos/ui/molecules'
  import { ListSection, NavigationBar } from '@agentos/ui/organisms'
  import { SplitViewTemplate, ScreenTemplate } from '@agentos/ui/templates'
  import ColorSwatch from '../atoms/ColorSwatch.svelte'
  import { settings, accents, type Theme } from '../lib/settings.svelte'

  type Pane = 'general' | 'appearance' | 'agents' | 'notifications' | 'privacy'

  const panes: { id: Pane; title: string; icon: IconName; color: string }[] = [
    { id: 'general', title: 'General', icon: 'gear', color: 'var(--ios-gray)' },
    { id: 'appearance', title: 'Display & Appearance', icon: 'sun', color: 'var(--ios-blue)' },
    { id: 'agents', title: 'Agents', icon: 'sparkles', color: 'var(--ios-purple)' },
    { id: 'notifications', title: 'Notifications', icon: 'bell', color: 'var(--ios-red)' },
    { id: 'privacy', title: 'Privacy & Security', icon: 'lock', color: 'var(--ios-blue)' }
  ]

  let pane = $state<Pane>('appearance')
  let query = $state('')
  const current = $derived(panes.find((p) => p.id === pane)!)
  const visiblePanes = $derived(
    panes.filter((p) => p.title.toLowerCase().includes(query.toLowerCase()))
  )

  // Local demo state
  let launchAtLogin = $state(true)
  let backgroundAgents = $state(true)
  let model = $state('claude-opus-5-5')
  let autonomy = $state<'ask' | 'edit' | 'full'>('ask')
  let webAccess = $state(true)
  let shellAccess = $state(false)
  let notify = $state({ allow: true, sounds: true, badges: true, summary: false })
  let confirmReset = $state(false)

  const models = [
    { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', note: 'Most capable' },
    { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', note: 'Balanced' },
    { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', note: 'Fastest' }
  ]

  const versions = hostBridge()?.versions ?? {}
</script>

<SplitViewTemplate sidebarWidth={300}>
  {#snippet sidebar()}
    <NavigationBar testid="settings-sidebar-nav" title="Settings" large>
      {#snippet accessory()}
        <SearchField testid="settings-search" bind:value={query} />
      {/snippet}
    </NavigationBar>

    <ListSection sidebar>
      <ListRow testid="settings-account" title="AgentOS User" subtitle="Account, Sync & Agents">
        {#snippet leading()}
          <IconTile size={44} background="linear-gradient(180deg, #c7c7cc, #8e8e93)">
            <Icon name="person" size={26} color="#fff" />
          </IconTile>
        {/snippet}
      </ListRow>
    </ListSection>

    <ListSection sidebar>
      {#each visiblePanes as p (p.id)}
        <ListRow
          testid="settings-pane-{p.id}"
          title={p.title}
          icon={p.icon}
          iconColor={p.color}
          selected={pane === p.id}
          onclick={() => (pane = p.id)}
        />
      {/each}
    </ListSection>
  {/snippet}

  {#snippet detail()}
    <ScreenTemplate>
      {#snippet navigationBar()}
        <NavigationBar testid="settings-nav" title={current.title} material />
      {/snippet}

      {#snippet content()}
        <div class="pane">
          {#if pane === 'general'}
            <ListSection header="About">
              <ListRow testid="settings-about-name" title="Name" value="AgentOS" />
              <ListRow testid="settings-about-version" title="Version" value="0.1.0" />
              <ListRow
                testid="settings-about-electron"
                title="Electron"
                value={versions.electron ?? '—'}
              />
              <ListRow
                testid="settings-about-chromium"
                title="Chromium"
                value={versions.chrome ?? '—'}
              />
            </ListSection>
            <ListSection footer="Background agents keep running when their window is closed.">
              <ListRow testid="settings-launch-at-login-row" title="Launch at Login">
                {#snippet trailing()}<Toggle
                    testid="settings-launch-at-login"
                    bind:checked={launchAtLogin}
                    label="Launch at Login"
                  />{/snippet}
              </ListRow>
              <ListRow testid="settings-background-agents-row" title="Background Agents">
                {#snippet trailing()}<Toggle
                    testid="settings-background-agents"
                    bind:checked={backgroundAgents}
                    label="Background Agents"
                  />{/snippet}
              </ListRow>
            </ListSection>
          {:else if pane === 'appearance'}
            <ListSection header="Appearance">
              <ListRow testid="settings-theme-row" title="Theme">
                {#snippet trailing()}
                  <SegmentedControl
                    testid="settings-theme"
                    label="Theme"
                    bind:value={settings.theme}
                    segments={[
                      { value: 'light' as Theme, label: 'Light' },
                      { value: 'dark' as Theme, label: 'Dark' },
                      { value: 'auto' as Theme, label: 'Automatic' }
                    ]}
                  />
                {/snippet}
              </ListRow>
              <ListRow testid="settings-accent-row" title="Accent Color">
                {#snippet trailing()}
                  <span class="swatches">
                    {#each accents as a (a.name)}
                      <ColorSwatch
                        testid="settings-accent-{a.name.toLowerCase()}"
                        color={a.value}
                        label={a.name}
                        size={22}
                        selected={settings.accent === a.value}
                        onclick={() => (settings.accent = a.value)}
                      />
                    {/each}
                  </span>
                {/snippet}
              </ListRow>
            </ListSection>
            <ListSection header="Text Size">
              <div class="slider-row">
                <IconSlider
                  testid="settings-text-size"
                  bind:value={settings.textScale}
                  min={85}
                  max={125}
                  step={5}
                  minIcon="doc"
                  maxIcon="doc"
                  label="Text size"
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
          {:else if pane === 'agents'}
            <ListSection header="Default Model">
              {#each models as m (m.id)}
                <ListRow
                  testid="settings-model-{m.id}"
                  title={m.name}
                  subtitle={m.note}
                  onclick={() => (model = m.id)}
                >
                  {#snippet trailing()}
                    {#if model === m.id}<Icon
                        name="checkmark"
                        size={18}
                        weight={2.6}
                        color="var(--tint)"
                      />{/if}
                  {/snippet}
                </ListRow>
              {/each}
            </ListSection>
            <ListSection
              header="Autonomy"
              footer="Controls how much an agent can do before asking you for approval."
            >
              <div class="segment-row">
                <SegmentedControl
                  testid="settings-autonomy"
                  block
                  label="Autonomy"
                  bind:value={autonomy}
                  segments={[
                    { value: 'ask', label: 'Ask First' },
                    { value: 'edit', label: 'Auto-Edit' },
                    { value: 'full', label: 'Full Auto' }
                  ]}
                />
              </div>
            </ListSection>
            <ListSection header="Capabilities">
              <ListRow
                testid="settings-web-access-row"
                title="Web Access"
                icon="globe"
                iconColor="var(--ios-blue)"
              >
                {#snippet trailing()}<Toggle
                    testid="settings-web-access"
                    bind:checked={webAccess}
                    label="Web Access"
                  />{/snippet}
              </ListRow>
              <ListRow
                testid="settings-shell-access-row"
                title="Shell Access"
                icon="terminal"
                iconColor="var(--ios-gray)"
              >
                {#snippet trailing()}<Toggle
                    testid="settings-shell-access"
                    bind:checked={shellAccess}
                    label="Shell Access"
                  />{/snippet}
              </ListRow>
            </ListSection>
            <ListSection>
              <ListRow
                testid="settings-reset-agents"
                title="Reset All Agents"
                destructive
                onclick={() => (confirmReset = true)}
              />
            </ListSection>
          {:else if pane === 'notifications'}
            <ListSection>
              <ListRow testid="settings-allow-notifications-row" title="Allow Notifications">
                {#snippet trailing()}<Toggle
                    testid="settings-allow-notifications"
                    bind:checked={notify.allow}
                    label="Allow Notifications"
                  />{/snippet}
              </ListRow>
            </ListSection>
            <ListSection header="Alerts">
              <ListRow testid="settings-sounds-row" title="Sounds">
                {#snippet trailing()}<Toggle
                    testid="settings-sounds"
                    bind:checked={notify.sounds}
                    disabled={!notify.allow}
                    label="Sounds"
                  />{/snippet}
              </ListRow>
              <ListRow testid="settings-badges-row" title="Badges">
                {#snippet trailing()}<Toggle
                    testid="settings-badges"
                    bind:checked={notify.badges}
                    disabled={!notify.allow}
                    label="Badges"
                  />{/snippet}
              </ListRow>
              <ListRow testid="settings-scheduled-summary-row" title="Scheduled Summary">
                {#snippet trailing()}<Toggle
                    testid="settings-scheduled-summary"
                    bind:checked={notify.summary}
                    disabled={!notify.allow}
                    label="Scheduled Summary"
                  />{/snippet}
              </ListRow>
            </ListSection>
          {:else}
            <ListSection>
              <ListRow
                testid="settings-privacy-location"
                title="Location Services"
                icon="globe"
                iconColor="var(--ios-blue)"
                value="Off"
                chevron
                onclick={() => {}}
              />
              <ListRow
                testid="settings-privacy-files"
                title="Files & Folders"
                icon="folder"
                iconColor="var(--ios-cyan)"
                chevron
                onclick={() => {}}
              />
              <ListRow
                testid="settings-privacy-microphone"
                title="Microphone"
                icon="mic"
                iconColor="var(--ios-orange)"
                chevron
                onclick={() => {}}
              />
            </ListSection>
            <ListSection header="Agent Permissions">
              <ListRow
                testid="settings-privacy-audit-log"
                title="Audit Log"
                icon="doc"
                iconColor="var(--ios-gray)"
                chevron
                onclick={() => {}}
              />
              <ListRow
                testid="settings-privacy-network"
                title="Network Activity"
                icon="network"
                iconColor="var(--ios-green)"
                chevron
                onclick={() => {}}
              />
            </ListSection>
          {/if}
        </div>
      {/snippet}
    </ScreenTemplate>
  {/snippet}
</SplitViewTemplate>

<Alert
  testid="settings-reset-alert"
  bind:open={confirmReset}
  title="Reset All Agents?"
  message="This stops every running agent and clears their memory. This can't be undone."
  actions={[
    { label: 'Cancel', style: 'cancel' },
    { label: 'Reset', style: 'destructive' }
  ]}
/>

<style>
  .pane {
    max-width: 720px;
    margin: 0 auto;
    padding-top: 16px;
  }
  .swatches {
    display: flex;
    gap: 10px;
    padding: 4px 0;
  }
  .slider-row,
  .segment-row {
    padding: 8px 16px;
  }
</style>
