<!-- @atom SpanBar — where a span sits on its trace's time line: a track the width of the trace and a bar from its start to its end, in its source's tint -->
<script lang="ts">
  interface Props {
    /** Where the span starts, as a share of the trace (0..1). */
    left: number
    /** How long it lasts, as a share of the trace (0..1). */
    width: number
    /** Its source's tint. */
    tint: string
    /** It failed: the bar is drawn in red. */
    failed?: boolean
  }

  let { left, width, tint, failed = false }: Props = $props()
</script>

<span class="track" aria-hidden="true">
  <span class="bar" style:left="{left * 100}%" style:width="{width * 100}%" style:background={failed ? 'var(--ios-red)' : tint}></span>
</span>

<style>
  .track {
    position: relative;
    display: block;
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--fill-tertiary);
  }
  .bar {
    position: absolute;
    top: 0;
    bottom: 0;
    min-width: 3px;
    border-radius: var(--radius-pill);
  }
</style>
