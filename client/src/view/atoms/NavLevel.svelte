<!--
  @atom NavLevel — provides where its children sit (landmark, back button, sidebar toggle) to the screens inside:
  a pushed screen of a navigation stack, or a column of a split view. Renders no element of its own.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { provideColumn, provideNavLevel, type ColumnContext, type NavLevel } from '../context'

  interface Props {
    level: NavLevel
    /** Set for a split view column: lets a chosen row move the collapsed flow forward. */
    column?: ColumnContext
    children: Snippet
  }

  let { level, column, children }: Props = $props()
  // The level is a live object (its getters read reactive state), so providing the reference once is enough.
  // svelte-ignore state_referenced_locally
  provideNavLevel(level)
  // svelte-ignore state_referenced_locally
  if (column) provideColumn(column)
</script>

{@render children()}
