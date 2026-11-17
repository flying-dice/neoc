<!--
  @molecule SearchField — UISearchBar. Atoms: Icon, TextInput.
  Test ids: `testid` on the input, `${testid}-clear`, `${testid}-cancel`.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import TextInput from '../atoms/TextInput.svelte'

  interface Props {
    testid: string
    value?: string
    placeholder?: string
    /** Show a "Cancel" button while focused, like UISearchBar. */
    showCancel?: boolean
    /** `small` is the compact macOS sidebar/toolbar search field. */
    size?: 'regular' | 'small'
    label?: string
  }

  let {
    testid,
    value = $bindable(''),
    placeholder = 'Search',
    showCancel = false,
    size = 'regular',
    label = 'Search'
  }: Props = $props()

  let focused = $state(false)
  let input: HTMLInputElement | undefined = $state()

  function clear(): void {
    value = ''
    input?.focus()
  }
  function cancel(): void {
    value = ''
    input?.blur()
  }
</script>

<div class="ios-search" class:small={size === 'small'}>
  <label class="field">
    <Icon name="magnifyingglass" size={size === 'small' ? 13 : 17} weight={2.2} />
    <TextInput
      {testid}
      bind:ref={input}
      bind:value
      type="search"
      {placeholder}
      aria-label={label}
      onfocus={() => (focused = true)}
      onblur={() => (focused = false)}
    />
    {#if value}
      <button type="button" class="clear" aria-label="Clear" data-testid="{testid}-clear" onclick={clear}>
        <Icon name="xmark" size={10} weight={3.2} />
      </button>
    {/if}
  </label>
  {#if showCancel && (focused || value)}
    <button type="button" class="cancel" data-testid="{testid}-cancel" onmousedown={(e) => e.preventDefault()} onclick={cancel}>
      Cancel
    </button>
  {/if}
</div>

<style>
  .ios-search {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    --placeholder: var(--label-secondary);
  }
  .field {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 36px;
    padding: 0 8px;
    border-radius: var(--radius-md);
    background: var(--fill-tertiary);
    color: var(--label-secondary);
    cursor: text;
  }
  .small .field {
    height: 28px;
    gap: 5px;
    border-radius: 7px;
  }
  .small .field :global(.ios-text-input) {
    font: 400 13px/16px var(--font-text);
    letter-spacing: 0;
  }
  .clear {
    display: grid;
    place-items: center;
    width: 17px;
    height: 17px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--label-tertiary);
    color: var(--bg);
    cursor: pointer;
  }
  .cancel {
    border: 0;
    background: none;
    padding: 0;
    color: var(--tint);
    font: var(--type-body);
    cursor: pointer;
  }
</style>
