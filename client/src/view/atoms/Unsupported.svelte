<!--
  @atom Unsupported — the placeholder for a component this client's kit version does not know:
  visible, labelled, harmless (`role="note"`). A node's own `fallback` slot is preferred; this is the last resort.
-->
<script lang="ts">
  interface Props {
    type: string
    testid: string
  }

  let { type, testid }: Props = $props()
</script>

<div class="unsupported" role="note" data-testid={testid} aria-label="Unsupported component {type}">
  Unsupported: {type}
</div>

<style>
  .unsupported {
    font: var(--type-footnote);
    color: var(--label-secondary);
    border: 1px dashed var(--separator);
    border-radius: var(--radius-sm);
    padding: 4px 8px;
  }
</style>
