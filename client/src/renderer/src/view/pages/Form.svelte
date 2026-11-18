<!--
  @page Form — an inset-grouped list of control rows (sugar for `List` with `insetGrouped`): its children, and the
  children of its `Section`s, are laid out as rows. A `form` landmark when it has a `label`. Templates: List.
-->
<script lang="ts">
  import List from '../templates/List.svelte'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
</script>

<List {node} {path} {scope} variant="form" />
