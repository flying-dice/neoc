<!-- @organism ServiceDetail — one service looked at: how it is doing, the buttons that start, stop, restart and remove it, what the operating system answered when it would not, the settings that can be changed, and what the manager says of it -->
<script lang="ts">
  type Manager = VosApp.Services.Manager
  type Restart = VosApp.Services.Restart
  type Service = VosApp.Services.Service
  import StatusDot from '../atoms/StatusDot.svelte'
  import type { Notice } from '../lib/services.svelte'
  import { isUp, restartLabel, stateLabel, toneOf, unsaid } from '../lib/format'

  interface Props {
    service: Service
    manager: Manager
    /** A call is in flight for it: the operating system may be asking its person. */
    busy?: boolean
    /** What the last change of it ended in, when it did not end well. */
    notice?: Notice | null
    /** Show the way back to the list (a window too narrow for it). */
    back?: boolean
    oncontrol: (action: 'start' | 'stop' | 'restart') => void
    onconfigure: (change: { autostart?: boolean; description?: string; restart?: Restart }) => void
    onremove: () => void
    onback?: () => void
  }

  let { service, manager, busy = false, notice = null, back = false, oncontrol, onconfigure, onremove, onback }: Props = $props()

  const up = $derived(isUp(service))
  const RESTARTS = (['never', 'on_failure', 'always'] as const).map((value) => ({ value, label: restartLabel(value) }))
  const canChange = (option: string): boolean => manager.configurable.includes(option)

  // the description is typed, then saved: what is typed is the window's until it is
  let typed = $state<string | null>(null)
  const description = $derived(typed ?? service.description ?? '')
  const changed = $derived(typed !== null && typed !== (service.description ?? ''))
  $effect(() => {
    void service.description
    typed = null
  })

  const TITLES: Readonly<Record<string, string>> = {
    denied: 'The operating system did not allow it',
    failed: 'It did not work',
    unsupported: 'Not supported here',
    invalid: 'It was refused',
    missing: 'It is gone'
  }
  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value

  /** What the manager says of one setting, or why it does not. */
  const said = (value: string | undefined, field: string): string => value ?? unsaid(manager, field)
</script>

<section class="detail" aria-label={service.label}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the services" onclick={onback} data-testid="services-detail-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Services
      </ui-button>
    {/if}
    <div class="heading">
      <h1 data-testid="services-detail-name">{service.label}</h1>
      <p class="status" data-testid="services-detail-state">
        <StatusDot tone={toneOf(service.state)} label={stateLabel(service.state)} />
        {stateLabel(service.state)}{service.stateDetail ? ` · ${service.stateDetail}` : ''}
      </p>
    </div>
  </header>
  <div class="actions">
    {#if busy}
      <ui-spinner label="Waiting for the operating system" data-testid="services-detail-busy"></ui-spinner>
      <span class="waiting" data-testid="services-detail-waiting">Waiting for the operating system…</span>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="gray" size="small" hit disabled={busy} onclick={() => oncontrol(up ? 'stop' : 'start')} data-testid="services-detail-{up ? 'stop' : 'start'}">
      <ui-icon slot="icon" name={up ? 'stop' : 'play'}></ui-icon>
      {up ? 'Stop' : 'Start'}
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="gray" size="small" hit disabled={busy} onclick={() => oncontrol('restart')} data-testid="services-detail-restart">Restart</ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button class="remove" variant="plain" size="small" destructive hit disabled={busy} onclick={onremove} data-testid="services-detail-remove">
      <ui-icon slot="icon" name="trash"></ui-icon>
      Unregister
    </ui-button>
  </div>
  {#if notice}
    <div class="notice">
      <ui-banner tone="error" data-testid="services-detail-notice" data-status={notice.status}>
        <strong>{TITLES[notice.status]}.</strong> {notice.message}
      </ui-banner>
    </div>
  {/if}
  <!-- the pane scrolls and may have nothing to focus (every control is disabled while a change is in flight): the keyboard scrolls it -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroll" tabindex="0" role="region" aria-label="Details of {service.label}">
    <ui-list variant="inset" label="Settings">
      <ui-section header="Settings" footer={canChange('autostart') ? undefined : 'This service manager does not change these settings.'}>
        <ui-row heading="Start Automatically">
          <ui-toggle
            slot="trailing"
            label="Start Automatically"
            hit
            checked={service.autostart === true}
            disabled={busy || !canChange('autostart') || service.autostart === undefined}
            onchange={(e: Event) => onconfigure({ autostart: (e.currentTarget as unknown as { checked: boolean }).checked })}
            data-testid="services-detail-autostart"
          ></ui-toggle>
        </ui-row>
        <ui-row heading="Restart">
          {#if canChange('restart') && service.restart !== undefined}
            <ui-segmented
              slot="trailing"
              options={RESTARTS}
              value={service.restart}
              disabled={busy}
              label="Restart"
              hit
              onchange={(e: Event) => onconfigure({ restart: valueOf(e) as Restart })}
              data-testid="services-detail-restart-policy"
            ></ui-segmented>
          {:else}
            <span slot="trailing" class="unsaid" data-testid="services-detail-restart-policy">
              {service.restart ? restartLabel(service.restart) : canChange('restart') ? 'Not Available' : 'Not Supported'}
            </span>
          {/if}
        </ui-row>
        <ui-row>
          <span class="key">Description</span>
          {#if canChange('description')}
            <ui-text-field
              variant="bare"
              label="Description"
              placeholder="What it is for"
              autocomplete="off"
              maxlength={1000}
              value={description}
              disabled={busy}
              oninput={(e: Event) => (typed = valueOf(e))}
              data-testid="services-detail-description"
            ></ui-text-field>
            {#if changed}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <ui-button variant="tinted" size="small" hit disabled={busy} onclick={() => onconfigure({ description })} data-testid="services-detail-description-save">Save</ui-button>
            {/if}
          {:else}
            <span class="unsaid" data-testid="services-detail-description">{service.description ?? unsaid(manager, 'description')}</span>
          {/if}
        </ui-row>
      </ui-section>
    </ui-list>
    <ui-list variant="inset" label="Definition">
      <ui-section header="Definition">
        <ui-row heading="Program" value={said(service.program ?? service.commandLine, 'program')} data-testid="services-detail-program"></ui-row>
        <ui-row heading="Arguments" value={service.args ? (service.args.length > 0 ? service.args.join(' ') : 'None') : unsaid(manager, 'args')} data-testid="services-detail-args"></ui-row>
        <ui-row heading="Working Directory" value={said(service.workingDirectory, 'working_directory')} data-testid="services-detail-directory"></ui-row>
        <ui-row
          heading="Environment"
          value={service.environment ? (service.environment.length > 0 ? service.environment.map((v) => `${v.name}=${v.value}`).join(' ') : 'None') : unsaid(manager, 'environment')}
          data-testid="services-detail-environment"
        ></ui-row>
        <ui-row heading="Runs As" value={said(service.user, 'user')} data-testid="services-detail-user"></ui-row>
        <ui-row heading="Defined In" value={said(service.path, 'path')} data-testid="services-detail-path"></ui-row>
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .detail {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 8px;
  }
  .heading {
    flex: 1;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 0 16px 8px;
  }
  .actions :global(.remove) {
    margin-inline-start: auto;
  }
  strong {
    font-weight: 600;
  }
  .waiting {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .notice {
    padding: 0 16px 8px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    --list-inset: 16px;
  }
  .key {
    flex: none;
    min-width: 96px;
  }
  .unsaid {
    color: var(--label-secondary);
  }
</style>
