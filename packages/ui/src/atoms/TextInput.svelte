<!-- @atom TextInput — bare, borderless text input used inside fields and rows. Test id: `testid`. -->
<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements'

  interface Props extends Omit<HTMLInputAttributes, 'value'> {
    testid: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    value?: string
    ref?: HTMLInputElement
  }

  let { testid, value = $bindable(''), ref = $bindable(), class: klass = '', ...rest }: Props = $props()
</script>

<input class="ios-text-input {klass}" bind:this={ref} bind:value data-testid={testid} {...rest} />

<style>
  .ios-text-input {
    flex: 1;
    min-width: 0;
    width: 100%;
    border: 0;
    outline: 0;
    padding: 0;
    background: transparent;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    caret-color: var(--tint);
    -webkit-user-select: text;
    user-select: text;
  }
  .ios-text-input::placeholder {
    color: var(--placeholder);
  }
  .ios-text-input::-webkit-search-cancel-button {
    display: none;
  }
</style>
