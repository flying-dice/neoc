<!-- @molecule SpanDetails — one span in full, as a section of rows: where it ran, how it ended, when and for how long, its ids and what it carried -->
<script lang="ts">
  import { SOURCE_LABEL, attributeRows, clock, duration, type Span } from '../lib/telemetry'

  interface Props {
    span: Span
  }

  let { span }: Props = $props()

  const STATUS = { ok: 'Succeeded', error: 'Failed', unset: 'Not Said' } as const
</script>

<ui-section header="Span · {span.name}" data-testid="telemetry-span-details">
  <ui-row heading="Ran In" value={SOURCE_LABEL[span.source]}></ui-row>
  <ui-row heading="Outcome" value={STATUS[span.status]} destructive={span.status === 'error'}></ui-row>
  <ui-row heading="Started" value={clock(span.startMs)}></ui-row>
  <ui-row heading="Duration" value={duration(span.endMs - span.startMs)}></ui-row>
  <ui-row heading="Span ID" value={span.spanId}></ui-row>
  {#if span.parentSpanId}<ui-row heading="Parent" value={span.parentSpanId}></ui-row>{/if}
  {#each attributeRows(span.attributes) as [name, value] (name)}
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
  .mono {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
</style>
