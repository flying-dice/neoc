<!-- @molecule TextFieldRow — form row with an optional leading label and a text input. Atoms: TextInput. Test id: `testid` on the input. -->
<script lang="ts">
  import TextInput from '../atoms/TextInput.svelte'

  interface Props {
    testid: string
    value?: string
    label?: string
    placeholder?: string
    type?: 'text' | 'email' | 'password' | 'url'
  }


  let { testid, value = $bindable(''), label, placeholder, type = 'text' }: Props = $props()
</script>

<label class="ios-text-field-row">
  {#if label}<span class="label">{label}</span>{/if}
  <TextInput {testid} bind:value {placeholder} {type} aria-label={label ?? placeholder} />
</label>

<style>
  .ios-text-field-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--row-height);
    padding: 0 var(--row-inset);
    position: relative;
    cursor: text;
  }
  .ios-text-field-row::after {
    content: '';
    position: absolute;
    left: var(--row-inset);
    right: 0;
    bottom: 0;
    height: var(--hairline);
    background: var(--list-separator, var(--separator));
  }
  .ios-text-field-row:last-child::after {
    display: none;
  }
  .label {
    flex: none;
    min-width: 90px;
  }
</style>
