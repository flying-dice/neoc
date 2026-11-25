<!-- @organism LogInspector — one log of the Logs view in full: what it says, its fields, and the way to the trace it belongs to -->
<script lang="ts">
  import LogDetails from '../molecules/LogDetails.svelte'
  import { SEVERITY_LABEL, type Log } from '../lib/telemetry'

  interface Props {
    log: Log
    back?: boolean
    onback?: () => void
    /** Open the trace the log belongs to. */
    ontrace: (traceId: string) => void
  }

  let { log, back = false, onback, ontrace }: Props = $props()
</script>

<section class="inspector" aria-label="Log">
  {#if back}
    <header class="bar">
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the logs" onclick={onback} data-testid="telemetry-log-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Logs
      </ui-button>
    </header>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroll" tabindex="0" role="region" aria-label="Details of the log">
    <div class="head">
      <h1 class="ios-large-title" class:ios-red={log.severity === 'error'} data-testid="telemetry-log-severity">{SEVERITY_LABEL[log.severity]}</h1>
      {#if log.traceId}
        <div class="actions">
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <ui-button variant="tinted" onclick={() => log.traceId && ontrace(log.traceId)} data-testid="telemetry-log-trace">
            <ui-icon slot="icon" name="network" size="16"></ui-icon>
            Show Trace
          </ui-button>
        </div>
      {:else}
        <p class="ios-footnote ios-secondary">This log was written outside any call, so it has no trace.</p>
      {/if}
    </div>
    <ui-list variant="insetGrouped" label="Log" data-testid="telemetry-log">
      <LogDetails {log} header="Details" />
    </ui-list>
  </div>
</section>

<style>
  .inspector {
    box-sizing: border-box;
    height: 100%;
    min-width: 0;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
  }
  .bar {
    flex: none;
    padding: 0 16px 4px 12px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding-bottom: 24px;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 20px 8px;
  }
  .head h1,
  .head p {
    margin: 0;
  }
  .actions {
    display: flex;
  }
</style>
