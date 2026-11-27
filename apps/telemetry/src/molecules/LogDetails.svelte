<!-- @molecule LogDetails — one log in full, as a section of rows: how severe, when, where and by which app it was written, the trace it belongs to, and its fields -->
<script lang="ts">
  import { SEVERITY_LABEL, SOURCE_LABEL, attributeRows, clock, type Log } from '../lib/telemetry'

  interface Props {
    log: Log
    /** The section's header. */
    header?: string
  }

  let { log, header = 'Log' }: Props = $props()

  const fields = $derived(attributeRows(log.attributes))
</script>

<ui-section {header} footer={log.attributes['vos.correlated'] === 'last-call' ? 'The page put this in the trace of the call that answered just before it.' : undefined} data-testid="telemetry-log-details">
  <ui-row>
    <div class="stack">
      <span class="ios-footnote ios-secondary">Message</span>
      <span class="ios-body message" class:ios-red={log.severity === 'error'}>{log.body || '(no message)'}</span>
    </div>
  </ui-row>
  <ui-row heading="Severity" value={SEVERITY_LABEL[log.severity]}></ui-row>
  <ui-row heading="Written" value={clock(log.timeMs)}></ui-row>
  <ui-row heading="Written By" value={SOURCE_LABEL[log.source]}></ui-row>
  {#if log.app}<ui-row heading="App" value={log.app}></ui-row>{/if}
  {#each fields as [name, value] (name)}
    <ui-row>
      <div class="stack">
        <span class="ios-body">{name}</span>
        <span class="ios-footnote ios-secondary mono">{value}</span>
      </div>
    </ui-row>
  {/each}
</ui-section>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding-block: 2px;
  }
  .message {
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
</style>
