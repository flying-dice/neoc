<!-- @atom Card — the rounded widget / platter surface. Optional test id: `testid`. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    /** Optional: cards are not interactive, but widgets and listings name themselves. */
    testid?: string
    /** Widget sizes follow iPadOS small / medium / large proportions. */
    size?: 'small' | 'medium' | 'large' | 'auto'
    surface?: 'solid' | 'material'
    children: Snippet
  }

  let {
    testid,
    size = 'auto',
    surface = 'solid',
    children
  }: Props = $props()
</script>

<div class="ios-card {size} {surface}" data-testid={testid}>
  {@render children()}
</div>

<style>
  .ios-card {
    position: relative;
    padding: 16px;
    border-radius: var(--radius-2xl);
    overflow: hidden;
    color: var(--label);
    box-shadow: var(--shadow-card);
  }
  .solid {
    background: var(--bg-elevated);
  }
  .material {
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
    background: var(--material-regular);
  }
  .small {
    width: 170px;
    height: 170px;
  }
  .medium {
    width: 364px;
    height: 170px;
  }
  .large {
    width: 364px;
    height: 382px;
  }
</style>
