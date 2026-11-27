<!-- @molecule SpanRow — one span of a trace's waterfall: its name, set in by how deep it sits, its bar on the trace's time line, and how long it took -->
<script lang="ts">
  import SpanBar from '../atoms/SpanBar.svelte'
  import { SOURCE_LABEL, SOURCE_TINT, duration, type Bar } from '../lib/telemetry'

  interface Props {
    bar: Bar
    selected?: boolean
    onpress: () => void
  }

  let { bar, selected = false, onpress }: Props = $props()

  const span = $derived(bar.span)
  const failed = $derived(span.status === 'error')
  const took = $derived(duration(span.endMs - span.startMs))
</script>

<ui-row
  value={took}
  label="{span.name} {took}, {SOURCE_LABEL[span.source]}{failed ? ', failed' : ''}"
  {selected}
  {onpress}
  data-testid="telemetry-span-{span.spanId}"
>
  <!-- only the name is set in: every bar is on the same time line, the width of the row -->
  <div class="span">
    <span class="name ios-body" class:ios-red={failed} style:padding-left="{bar.depth * 14}px">
      <span class="dot" style:background={SOURCE_TINT[span.source]}></span>{span.name}
    </span>
    <SpanBar left={bar.left} width={bar.width} tint={SOURCE_TINT[span.source]} {failed} />
  </div>
</ui-row>

<style>
  .span {
    box-sizing: border-box;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding-block: 2px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
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
