<!-- @molecule SpanRow — one span of a trace's waterfall: its name, set in by how deep it sits, its bar on the trace's time line, and how long it took; a failed one says why under its name -->
<script lang="ts">
  import SpanBar from '../atoms/SpanBar.svelte'
  import { SOURCE_LABEL, SOURCE_TINT, attributeText, duration, type Bar } from '../lib/telemetry'

  interface Props {
    bar: Bar
    selected?: boolean
    onpress: () => void
  }

  let { bar, selected = false, onpress }: Props = $props()

  const span = $derived(bar.span)
  const failed = $derived(span.status === 'error')
  /** Why it failed, as the span recorded it: what a failed span is opened to find out, said in the row. */
  const why = $derived(failed ? attributeText(span.attributes.error ?? span.attributes['error.message'] ?? '') : '')
  const took = $derived(duration(span.endMs - span.startMs))
</script>

<ui-row
  value={took}
  label="{span.name}{why ? ` ${why}` : ''} {took}, {SOURCE_LABEL[span.source]}{failed ? ', failed' : ''}"
  {selected}
  {onpress}
  data-testid="telemetry-span-{span.spanId}"
>
  <!-- only the name is set in: every bar is on the same time line, the width of the row -->
  <div class="span">
    <span class="name ios-body" class:ios-red={failed} style:padding-left="{bar.depth * 16}px">
      <span class="dot" style:background={SOURCE_TINT[span.source]}></span>{span.name}
    </span>
    {#if why}<span class="why ios-footnote ios-red" style:padding-left="{bar.depth * 16 + 16}px">{why}</span>{/if}
    <SpanBar left={bar.left} width={bar.width} tint={SOURCE_TINT[span.source]} {failed} />
  </div>
</ui-row>

<style>
  .span {
    box-sizing: border-box;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    padding-block: 2px;
  }
  .why {
    overflow-wrap: anywhere;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
