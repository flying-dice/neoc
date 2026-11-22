<!-- @molecule Composer — a field something new is named in, with the button that adds it, and under the field what is wrong with the name when it cannot be had -->
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
    /** What is wrong with what is typed: said under the field, which is marked. The problem's test id is the field's with `-problem` in place of `-input`. */
    problem?: string | null
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { draft, label, action, testid, problem = null, ondraft, onadd }: Props = $props()
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
    invalid={problem !== null}
    description={problem ?? undefined}
    value={draft}
    oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
    data-testid="{testid}-input"
  ></ui-text-field>
  <ui-button type="submit" square shape="circle" hit label={action} disabled={draft.trim() === ''} data-testid="{testid}-add">
    <ui-icon slot="icon" name="plus"></ui-icon>
  </ui-button>
  {#if problem !== null}
    <p class="problem" data-testid="{testid}-problem">{problem}</p>
  {/if}
</form>

<style>
  .composer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .problem {
    flex: 0 0 100%;
    margin: 0;
    padding: 0 4px;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
