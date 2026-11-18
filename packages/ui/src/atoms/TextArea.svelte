<!-- @atom TextArea — bare multi-line text input (the body of a TextEditor). Test id: `testid`. -->
<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements'

  interface Props extends Omit<HTMLTextareaAttributes, 'value'> {
    testid: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    value?: string
    ref?: HTMLTextAreaElement
    /** Monospace body (code, logs). */
    mono?: boolean
  }

  let {
    testid,
    value = $bindable(''),
    ref = $bindable(),
    mono = false,
    class: klass = '',
    ...rest
  }: Props = $props()
</script>

<textarea
  class="ios-text-area {klass}"
  class:mono
  bind:this={ref}
  bind:value
  data-testid={testid}
  {...rest}
></textarea>

<style>
  .ios-text-area {
    display: block;
    flex: 1;
    min-width: 0;
    width: 100%;
    min-height: 0;
    resize: none;
    border: 0;
    outline: 0;
    margin: 0;
    padding: 0;
    background: transparent;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    caret-color: var(--tint);
    -webkit-user-select: text;
    user-select: text;
    overflow-wrap: anywhere;
  }
  .mono {
    font: var(--type-callout);
    font-family: var(--font-mono);
    letter-spacing: 0;
  }
  .ios-text-area::placeholder {
    color: var(--label-secondary);
  }
</style>
