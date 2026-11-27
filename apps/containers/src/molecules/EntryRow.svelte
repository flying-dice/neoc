<!-- @molecule EntryRow — one name and its value in a list of the form (a variable of the environment, a label), with the button that takes it out -->
<script lang="ts">
  type Entry = VosApp.Containers.Entry

  interface Props {
    /** The row, changed as it is typed in. */
    entry: Entry
    /** What a row is, in the names of its fields and its button: `Variable`, `Label`. */
    kind: string
    /** What the name looks like, as a hint. */
    placeholder?: string
    disabled?: boolean
    /** The row's test id; its fields' and button's follow from it (`-name`, `-value`, `-remove`). */
    testid: string
    onremove: () => void
  }

  let { entry = $bindable(), kind, placeholder = 'NAME', disabled = false, testid, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
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
      {disabled}
      value={entry.name}
      oninput={(e: Event) => (entry.name = valueOf(e))}
      data-testid="{testid}-name"
    ></ui-text-field>
    <span class="equals" aria-hidden="true">=</span>
    <ui-text-field
      class="value"
      label="{kind} value"
      placeholder="value"
      autocapitalize="off"
      autocomplete="off"
      variant="bare"
      {disabled}
      value={entry.value}
      oninput={(e: Event) => (entry.value = valueOf(e))}
      data-testid="{testid}-value"
    ></ui-text-field>
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
    {disabled}
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
    gap: 8px;
  }
  .name {
    flex: 1 1 40%;
    min-width: 0;
    font-family: var(--font-mono);
  }
  .value {
    flex: 1 1 60%;
    min-width: 0;
  }
  .equals {
    flex: none;
    color: var(--label-tertiary);
  }
</style>
