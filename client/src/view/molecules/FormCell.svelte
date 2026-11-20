<!--
  @molecule FormCell — a node inside a `Form` made to look like a row: a row-height cell with the hairline separator
  of its siblings. Rows and row-like components (Row, NavigationLink, LabeledContent, DisclosureGroup) already are
  rows and render as they are; controls (toggles, fields, pickers, buttons) get the cell. Atoms: Children.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import type { Node } from '../protocol'
  import type { Scope } from '../core/view'

  interface Props {
    node: Node
    path: string
    scope: Scope
  }

  let { node, path, scope }: Props = $props()

  const ROWLIKE = new Set([
    'Row',
    'NavigationLink',
    'LabeledContent',
    'DisclosureGroup',
    'Section',
    'EmptyState',
    'Divider'
  ])
  const plain = $derived(ROWLIKE.has(node.type))
</script>

{#if plain}
  <Children nodes={[node]} {path} {scope} />
{:else}
  <div class="cell">
    <Children nodes={[node]} {path} {scope} />
  </div>
{/if}

<style>
  .cell {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-height: var(--row-height);
    padding: 6px var(--row-inset);
    min-width: 0;
  }
  .cell::after {
    content: '';
    position: absolute;
    left: var(--row-inset);
    right: 0;
    bottom: calc(var(--hairline) * -1);
    height: var(--hairline);
    background: var(--list-separator, var(--separator));
  }
</style>
