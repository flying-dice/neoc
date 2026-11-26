<!-- @organism RegisterForm — the next service as it is typed, as a form of iPadOS is: a row each for its name, the program, the arguments, what it is for, the directory it starts in, whether it starts by itself and what happens when it ends and how long it waits; what the operating system refused in the footer; settings the manager does not keep are said so -->
<script lang="ts">
  type Manager = VosApp.Services.Manager
  type Restart = VosApp.Services.Restart
  import type { Draft, Notice } from '../lib/services.svelte'
  import { restartLabel } from '../lib/format'

  interface Props {
    draft: Draft
    manager: Manager
    /** There is something to register: a name and a program. */
    ready: boolean
    /** The service is being registered: the operating system may be asking its person. */
    busy?: boolean
    /** What the last attempt ended in, when it did not end well. */
    notice?: Notice | null
    /** Show the way back to the list (a window too narrow for it). */
    back?: boolean
    ondraft: (change: Partial<Draft>) => void
    onregister: () => void
    oncancel: () => void
  }

  let { draft, manager, ready, busy = false, notice = null, back = false, ondraft, onregister, oncancel }: Props = $props()

  const keeps = (option: string): boolean => manager.installOptions.includes(option)
  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const RESTARTS = [{ value: '', label: 'Default' }, ...(['never', 'on_failure', 'always'] as const).map((value) => ({ value, label: restartLabel(value) }))]
  // what was refused is said under the rows; denied is the operating system's, not a slip of typing
  const footer = $derived(notice ? notice.message : 'The program must be the absolute path of an executable file. It is registered, not started.')
</script>

<form
  class="form"
  novalidate
  onsubmit={(e) => {
    e.preventDefault()
    if (ready && !busy) onregister()
  }}
>
  <header class="bar">
    {#if back}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the services" onclick={oncancel} data-testid="services-form-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Services
      </ui-button>
    {/if}
    <h1>Register Service</h1>
  </header>
  <div class="scroll">
    <ui-list variant="inset" label="A new service">
      <ui-section footer={footer} footer-tone={notice ? 'error' : 'note'} data-testid="services-form">
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field variant="bare" label="Name" placeholder="my-app" autocomplete="off" autocapitalize="off" maxlength={200} value={draft.label} oninput={(e: Event) => ondraft({ label: valueOf(e) })} data-testid="services-form-label"></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Program</span>
          <ui-text-field variant="bare" label="Program" placeholder="/usr/local/bin/my-app" autocomplete="off" autocapitalize="off" maxlength={4000} value={draft.program} oninput={(e: Event) => ondraft({ program: valueOf(e) })} data-testid="services-form-program"></ui-text-field>
        </ui-row>
        {#if keeps('args')}
          <ui-row>
            <span class="key">Arguments</span>
            <ui-text-area variant="bare" label="Arguments, one a line" placeholder="One a line" rows={2} value={draft.args} oninput={(e: Event) => ondraft({ args: valueOf(e) })} data-testid="services-form-args"></ui-text-area>
          </ui-row>
        {/if}
        {#if keeps('description')}
          <ui-row>
            <span class="key">Description</span>
            <ui-text-field variant="bare" label="Description" placeholder="What it is for" autocomplete="off" maxlength={1000} value={draft.description} oninput={(e: Event) => ondraft({ description: valueOf(e) })} data-testid="services-form-description"></ui-text-field>
          </ui-row>
        {:else}
          <ui-row heading="Description" value="Not Supported" data-testid="services-form-description-unsupported"></ui-row>
        {/if}
        {#if keeps('working_directory')}
          <ui-row>
            <span class="key">Directory</span>
            <ui-text-field variant="bare" label="Working directory" placeholder="/var/lib/my-app" autocomplete="off" autocapitalize="off" maxlength={4000} value={draft.workingDirectory} oninput={(e: Event) => ondraft({ workingDirectory: valueOf(e) })} data-testid="services-form-directory"></ui-text-field>
          </ui-row>
        {/if}
        {#if keeps('environment')}
          <ui-row>
            <span class="key">Environment</span>
            <ui-text-area variant="bare" label="Environment, NAME=value one a line" placeholder="NAME=value, one a line" rows={2} value={draft.environment} oninput={(e: Event) => ondraft({ environment: valueOf(e) })} data-testid="services-form-environment"></ui-text-area>
          </ui-row>
        {:else}
          <ui-row heading="Environment" value="Not Supported" data-testid="services-form-environment-unsupported"></ui-row>
        {/if}
        {#if keeps('autostart')}
          <ui-row heading="Start automatically">
            <ui-toggle slot="trailing" label="Start automatically" hit checked={draft.autostart} onchange={(e: Event) => ondraft({ autostart: (e.currentTarget as unknown as { checked: boolean }).checked })} data-testid="services-form-autostart"></ui-toggle>
          </ui-row>
        {/if}
        {#if keeps('restart')}
          <ui-row heading="Restart">
            <ui-segmented slot="trailing" options={RESTARTS} value={draft.restart} label="Restart" hit onchange={(e: Event) => ondraft({ restart: valueOf(e) as Restart | '' })} data-testid="services-form-restart"></ui-segmented>
          </ui-row>
        {/if}
        {#if keeps('restart_delay_secs')}
          <ui-row>
            <span class="key">Restart delay</span>
            <ui-text-field variant="bare" label="Restart delay in seconds" placeholder="Seconds" inputmode="numeric" autocomplete="off" maxlength={5} value={draft.restartDelay} oninput={(e: Event) => ondraft({ restartDelay: valueOf(e) })} data-testid="services-form-restart-delay"></ui-text-field>
          </ui-row>
        {/if}
      </ui-section>
    </ui-list>
    <div class="buttons">
      {#if busy}<ui-spinner label="Registering the service" data-testid="services-form-busy"></ui-spinner>{/if}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" hit onclick={oncancel} data-testid="services-form-cancel">Cancel</ui-button>
      <ui-button type="submit" hit disabled={!ready || busy} data-testid="services-form-register">Register</ui-button>
    </div>
  </div>
</form>

<style>
  .form {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    /* the rows line up with the rest of the window */
    --list-inset: 16px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 8px;
  }
  h1 {
    margin: 0;
    font: var(--type-title2);
    font-weight: 700;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .key {
    flex: none;
    min-width: 96px;
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 8px 16px 16px;
  }
</style>
