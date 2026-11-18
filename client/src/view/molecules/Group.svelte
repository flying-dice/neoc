<!--
  @molecule Group — a transparent container: no layout of its own (`display: contents`). It holds presentation
  nodes and passes its children through. Atoms: Children.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
</script>

<div class="group">
  <Children nodes={node.children} {path} {scope} />
</div>

<style>
  .group {
    display: contents;
  }
</style>
