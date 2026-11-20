<!--
  @atom Scope — re-points the colour scheme, tint and background for a subtree: the app's own canvas (a
  Calculator's black body). `data-theme` switches every `light-dark()` token below it; `tint` is a palette name.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid?: string
    scheme?: 'light' | 'dark'
    /** A palette name (`blue`, `orange`, ...): re-points `--tint` for the subtree. */
    tint?: string
    /** Paint the scheme's page background (default: when a scheme is set). */
    paint?: boolean
    style?: string
    children: Snippet
  }

  let { testid, scheme, tint, paint, style, children }: Props = $props()
</script>

<div
  class="ios-scope"
  class:paint={paint ?? scheme !== undefined}
  data-testid={testid}
  data-theme={scheme}
  style:color-scheme={scheme}
  style:--tint={tint ? `var(--ios-${tint})` : undefined}
  {style}
>
  {@render children()}
</div>

<style>
  .ios-scope {
    display: flex;
    flex-direction: column;
    min-width: 0;
    color: var(--label);
  }
  .paint {
    background: var(--bg-grouped);
  }
</style>
