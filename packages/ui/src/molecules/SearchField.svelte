<!--
  @molecule SearchField — UISearchBar. Atoms: Icon, TextInput.
  Escape clears the text and leaves the field. Test ids: `testid` on the input, `${testid}-clear`.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import TextInput from '../atoms/TextInput.svelte'

  interface Props {
    testid: string
    value?: string
  }

  let { testid, value = $bindable('') }: Props = $props()

  let focused = $state(false)
  let input: HTMLInputElement | undefined = $state()

  function clear(): void {
    value = ''
    input?.focus()
  }
  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || e.key !== 'Escape' || !(value || focused)) return
    e.preventDefault()
    e.stopPropagation()
    value = ''
    input?.blur()
  }
</script>

<div class="ios-search">
  <label class="field">
    <Icon name="magnifyingglass" size={17} weight={2.2} />
    <TextInput
      {testid}
      bind:ref={input}
      bind:value
      type="search"
      placeholder="Search"
      aria-label="Search"
      onfocus={() => (focused = true)}
      onblur={() => (focused = false)}
      {onkeydown}
    />
    {#if value}
      <button type="button" class="clear" aria-label="Clear" data-testid="{testid}-clear" onclick={clear}>
        <Icon name="xmark" size={10} weight={3.2} />
      </button>
    {/if}
  </label>
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
</style>
