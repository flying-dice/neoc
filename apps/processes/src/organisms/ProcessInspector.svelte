<!-- @organism ProcessInspector — one process in full: its name and state, the two ways to end it (each asks first), why it was not ended, and what it uses, who runs it, how it was started, and the processes it is next to -->
<script lang="ts">
  type Detail = VosApp.Processes.Detail
  type Related = VosApp.Processes.Related
  import { bytes, commandLine, duration, percent, started, statusLabel } from '../lib/format'
  import type { Ending, Refusal } from '../lib/processes.svelte'

  interface Props {
    detail: Detail
    /** This system can ask a program to quit; without it only Force Terminate is offered. */
    canTerminate: boolean
    /** The process is still there and no call to end it is on its way. */
    endable: boolean
    /** Why a process that is there cannot be ended, when it cannot (it can still be looked at). */
    unavailable?: string | null
    /** The process is being ended. */
    ending?: boolean
    /** The question being asked before it is ended. */
    asking?: Ending | null
    /** Why it was not ended. */
    refused?: Refusal | null
    /** What was done to it. */
    notice?: string | null
    /** Show the way back to the list (a window too narrow for both). */
    back?: boolean
    onback?: () => void
    onask: (ending: Ending) => void
    onconfirm: () => void
    onkeep: () => void
    onfollow: (related: Related) => void
  }

  let { detail, canTerminate, endable, unavailable = null, ending = false, asking = null, refused = null, notice = null, back = false, onback, onask, onconfirm, onkeep, onfollow }: Props = $props()

  const state = $derived(detail.alive ? statusLabel(detail.status) : 'Quit')
  const command = $derived(commandLine(detail.command))
</script>

<section class="inspector" aria-label={detail.name}>
  {#if back}
    <header class="bar">
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the processes" onclick={onback} data-testid="processes-detail-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Processes
      </ui-button>
    </header>
  {/if}
  <!-- the pane scrolls and may have nothing to focus (a process that has quit): the keyboard scrolls it -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroll" tabindex="0" role="region" aria-label="Details of {detail.name}">
    <div class="head">
      <h1 class="ios-large-title" data-testid="processes-detail-name">{detail.name}</h1>
      <p class="ios-subheadline ios-secondary" data-testid="processes-detail-state">PID {detail.pid} · {state}</p>
      <div class="actions">
        {#if ending}<ui-spinner label="Ending {detail.name}" data-testid="processes-detail-busy"></ui-spinner>{/if}
        {#if canTerminate}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <ui-button variant="gray" size="large" disabled={!endable} onclick={() => onask('terminate')} data-testid="processes-terminate">
            <ui-icon slot="icon" name="stop" size="16"></ui-icon>
            Terminate…
          </ui-button>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button variant="gray" size="large" destructive disabled={!endable} onclick={() => onask('force')} data-testid="processes-force">
          Force Terminate…
        </ui-button>
      </div>
      <p class="ios-footnote ios-secondary note">
        {canTerminate ? 'Terminate asks it to quit. Force Terminate stops it at once.' : 'This system cannot ask a program to quit. Force Terminate stops it at once.'}
      </p>
      {#if unavailable}
        <ui-banner tone="warning" data-testid="processes-unavailable">{unavailable}</ui-banner>
      {/if}
      {#if refused}
        <ui-banner tone="error" data-testid="processes-refused">
          <strong>Can’t Terminate {refused.name}</strong>
          {refused.message}
        </ui-banner>
      {:else if notice}
        <ui-banner tone="info" data-testid="processes-notice">{notice}</ui-banner>
      {/if}
      {#if !detail.alive}
        <ui-banner tone="warning" data-testid="processes-quit">{detail.name} has quit. What is shown is what it was when it was last seen.</ui-banner>
      {/if}
    </div>

    <ui-list variant="insetGrouped" label="Details" data-testid="processes-detail">
      <ui-section header="Usage">
        <ui-row heading="CPU" value={percent(detail.cpu)} data-testid="processes-detail-cpu"></ui-row>
        <ui-row heading="Memory" value={detail.memory === undefined ? '—' : bytes(detail.memory)} data-testid="processes-detail-memory"></ui-row>
        {#if detail.virtualMemory !== undefined}
          <ui-row heading="Virtual Memory" value={bytes(detail.virtualMemory)} data-testid="processes-detail-virtual"></ui-row>
        {/if}
      </ui-section>
      <ui-section header="Process" footer={detail.alive && detail.cpu === undefined ? 'CPU needs two readings. It appears after the next refresh.' : undefined}>
        <ui-row heading="Status" value={state} data-testid="processes-detail-status"></ui-row>
        {#if detail.user}<ui-row heading="User" value={detail.user} data-testid="processes-detail-user"></ui-row>{/if}
        <ui-row heading="Started" value={started(detail.startTime)} data-testid="processes-detail-started"></ui-row>
        {#if detail.runTimeSecs !== undefined}
          <ui-row heading="Running For" value={duration(detail.runTimeSecs)} data-testid="processes-detail-runtime"></ui-row>
        {/if}
        {#if detail.exe}
          <ui-row data-testid="processes-detail-exe">
            <div class="stack">
              <span class="ios-body">Program</span>
              <span class="ios-footnote ios-secondary mono">{detail.exe}</span>
            </div>
          </ui-row>
        {/if}
        {#if command !== ''}
          <ui-row data-testid="processes-detail-command">
            <div class="stack">
              <span class="ios-body">Command</span>
              <span class="ios-footnote ios-secondary mono">{command}</span>
            </div>
          </ui-row>
        {/if}
      </ui-section>
      {#if detail.parent}
        <ui-section header="Parent">
          <ui-row
            tappable
            chevron
            heading={detail.parent.name}
            subheading={detail.parent.user ? `PID ${detail.parent.pid} · ${detail.parent.user}` : `PID ${detail.parent.pid}`}
            onpress={() => detail.parent && onfollow(detail.parent)}
            data-testid="processes-parent"
          ></ui-row>
        </ui-section>
      {/if}
      {#if detail.children.length > 0}
        <ui-section header="Children">
          {#each detail.children as child (`${child.pid}:${child.startTime}:${child.observation}`)}
            <ui-row tappable chevron heading={child.name} subheading="PID {child.pid}" onpress={() => onfollow(child)} data-testid="processes-child-{child.pid}"></ui-row>
          {/each}
        </ui-section>
      {/if}
    </ui-list>
  </div>
  {#if asking}
    <ui-alert
      open
      destructive={asking === 'force'}
      action-label={asking === 'force' ? 'Force Terminate' : 'Terminate'}
      heading="{asking === 'force' ? 'Force Terminate' : 'Terminate'} “{detail.name}”?"
      message={asking === 'force' ? 'It stops at once and cannot save its work.' : 'It is asked to quit. It may save its work first, or ignore the request.'}
      data-testid="processes-confirm"
      onconfirm={onconfirm}
      oncancel={onkeep}
    ></ui-alert>
  {/if}
</section>

<style>
  .inspector {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
  }
  .bar {
    flex: none;
    padding: 0 8px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .head {
    padding: 8px 20px 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .head h1,
  .head p {
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
  }
  .note {
    margin-top: 6px;
  }
  ui-banner {
    margin-top: 10px;
  }
  ui-list {
    padding-top: 12px;
  }
  .stack {
    flex: 1;
    min-width: 0;
    padding: 10px 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .mono {
    font-family: var(--font-mono);
    word-break: break-all;
  }
</style>
