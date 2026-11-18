<!--
  @atom LabeledContent — a row-height label with a value or content beside it (Settings: "Version  1.2.0"). The
  label wraps before the value does, so it never forces a horizontal scroll. Optional test id: `testid`.
-->
<script lang="ts">
  import { getContext, type Snippet } from 'svelte'
  import { LIST_CONTEXT } from '../logic/listContext'

  interface Props {
    testid?: string
    label: string
    value?: string
    style?: string
    children?: Snippet
  }

  let { testid, label, value, style, children }: Props = $props()
  const uid = $props.id()
  /** In a list or form the row has the hairline separator of its siblings. */
  const inList = getContext(LIST_CONTEXT) !== undefined
</script>

<div class="ios-labeled" class:in-list={inList} data-testid={testid} role="group" aria-labelledby="{uid}-label" {style}>
  <span class="label" id="{uid}-label">{label}</span>
  <span class="value">
    {#if children}{@render children()}{:else if value !== undefined}{value}{/if}
  </span>
</div>

<style>
  .ios-labeled {
    position: relative;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    column-gap: 12px;
    min-height: var(--row-height);
    padding: 6px var(--row-inset);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
  }
  .in-list::after {
    content: '';
    position: absolute;
    left: var(--row-inset);
    right: 0;
    bottom: calc(var(--hairline) * -1);
    height: var(--hairline);
    background: var(--list-separator, var(--separator));
  }
  .label {
    flex: 0 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .value {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    text-align: right;
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
</style>
