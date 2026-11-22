<!-- @molecule Composer — a field something new is named in, with the button that adds it: the next todo of a list, or the next list -->
<script lang="ts">
  interface Props {
    /** What is typed so far. */
    draft: string
    /** What the field is for: its accessible name, and its hint while it is empty. */
    label: string
    /** What the button does, for a screen reader. */
    action: string
    /** The test id of the field; the button's is this with `-add` in place of `-input`. */
    testid: string
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { draft, label, action, testid, ondraft, onadd }: Props = $props()
</script>

<form
  class="composer"
  onsubmit={(e) => {
    e.preventDefault()
    onadd()
  }}
>
  <ui-text-field
    {label}
    placeholder={label}
    enterkeyhint="done"
    autocomplete="off"
    hit
    value={draft}
    oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
    data-testid="{testid}-input"
  ></ui-text-field>
  <ui-button type="submit" square shape="circle" hit label={action} disabled={draft.trim() === ''} data-testid="{testid}-add">
    <ui-icon slot="icon" name="plus"></ui-icon>
  </ui-button>
</form>

<style>
  .composer {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
