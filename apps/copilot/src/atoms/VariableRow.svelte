<!-- @atom VariableRow — one name and its value in a list of the form (a variable of the environment, a header): the value written out or a Keychain item's password, with the button that takes the row out -->
<script lang="ts">
  type Secret = VosApp.Copilot.Secret
  import { secretKey, secretLabel, type VariableDraft } from '../lib/form'

  interface Props {
    /** The row, changed as it is typed in. */
    variable: VariableDraft
    /** What a row is, in the names of its fields and its button: `Variable`, `Header`. */
    kind: string
    /** What the name looks like, as a hint. */
    placeholder?: string
    /** The Keychain's items the value can be; empty when they could not be read. */
    secrets: readonly Secret[]
    /** The row's test id; its fields' and button's follow from it (`-name`, `-source`, `-value`, `-remove`). */
    testid: string
    onremove: () => void
  }

  let { variable = $bindable(), kind, placeholder = 'NAME', secrets, testid, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  // the item the row has stays a choice, though the Keychain no longer lists it (or could not be read)
  const choices = $derived.by(() => {
    const items = [...secrets]
    if (variable.secret && !items.some((s) => secretKey(s) === secretKey(variable.secret!))) items.unshift(variable.secret)
    return [{ value: '', label: 'Value' }, ...items.map((s) => ({ value: secretKey(s), label: secretLabel(s) }))]
  })

  function choose(value: string): void {
    const [service = '', account = ''] = value === '' ? [] : (JSON.parse(value) as string[])
    variable.secret = value === '' ? null : { service, account }
  }
</script>

<ui-row data-testid={testid}>
  <div class="entry">
    <ui-text-field
      class="name"
      label="{kind} name"
      {placeholder}
      autocapitalize="off"
      autocomplete="off"
      variant="bare"
      value={variable.name}
      oninput={(e: Event) => (variable.name = valueOf(e))}
      data-testid="{testid}-name"
    ></ui-text-field>
    <span class="equals" aria-hidden="true">=</span>
    {#if choices.length > 1}
      <ui-picker
        class="source"
        label="Where its value comes from"
        variant="bare"
        options={choices}
        value={variable.secret ? secretKey(variable.secret) : ''}
        onchange={(e) => choose(e.detail.value)}
        data-testid="{testid}-source"
      ></ui-picker>
    {/if}
    {#if !variable.secret}
      <ui-text-field
        class="value"
        label="{kind} value"
        placeholder="value"
        autocapitalize="off"
        autocomplete="off"
        variant="bare"
        value={variable.value}
        oninput={(e: Event) => (variable.value = valueOf(e))}
        data-testid="{testid}-value"
      ></ui-text-field>
    {/if}
  </div>
  <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <ui-button
    slot="trailing"
    variant="plain"
    square
    size="small"
    destructive
    hit
    label="Remove this {kind.toLowerCase()}"
    onclick={onremove}
    data-testid="{testid}-remove"
  >
    <ui-icon slot="icon" name="minus"></ui-icon>
  </ui-button>
</ui-row>

<style>
  .entry {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 8px;
  }
  .name {
    flex: 1 1 30%;
    min-width: 8em;
    font-family: var(--font-mono);
  }
  .source {
    flex: 0 1 auto;
    min-width: 0;
  }
  .value {
    flex: 1 1 40%;
    min-width: 8em;
  }
  .equals {
    flex: none;
    color: var(--label-tertiary);
  }
</style>
