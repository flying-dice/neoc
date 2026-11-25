<!-- @molecule UsageTile — one summary tile of the machine: what it is about, its value in large type, a note beside it, and a meter of how full it is (an empty one while there is no reading yet) -->
<script lang="ts">
  interface Props {
    /** What the tile is about (`CPU`). */
    heading: string
    /** The headline value (`34%`, `9.8 GB`); a dash while there is none. */
    value: string
    /** What goes beside the value (`8 cores`, `of 16 GB`). */
    note?: string
    /** How full, 0 to 100. Left out, the meter is empty: the reading has not been taken yet. */
    share?: number
    /** The test id of the tile; its parts add `-value` and `-note`. */
    testid: string
  }

  let { heading, value, note, share, testid }: Props = $props()
</script>

<ui-tile {heading} data-testid={testid}>
  <span class="ios-title2" data-testid="{testid}-value">{value}</span>
  {#if note}<span class="ios-footnote ios-secondary" data-testid="{testid}-note">{note}</span>{/if}
  <ui-meter slot="footer" label={heading} value={share}></ui-meter>
</ui-tile>
