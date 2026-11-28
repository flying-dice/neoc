<!-- @molecule Composer — a field something new is named in, with the button that adds it, and under it what is wrong with the name: the next folder -->
<script lang="ts">
  interface Props {
    /** What is typed so far. */
    draft: string
    /** What the field is for: its accessible name, and its hint while it is empty. */
    label: string
    /** What the button does, for a screen reader. */
    action: string
    /** What is wrong with what is typed; nothing can be added while there is something. */
    problem?: string | null
    /** The test id of the field; the button's is this with `-add` in place of `-input`, the problem's with `-problem`. */
    testid: string
    ondraft: (text: string) => void
    onadd: () => void
  }

  let { draft, label, action, problem = null, testid, ondraft, onadd }: Props = $props()
</script>

<form
  class="composer"
  onsubmit={(e) => {
    e.preventDefault()
    if (!problem) onadd()
  }}
>
  <div class="line">
    <ui-text-field
      {label}
      description={problem ?? undefined}
      invalid={problem !== null}
      placeholder={label}
      enterkeyhint="done"
      autocomplete="off"
      maxlength={60}
      hit
      value={draft}
      oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
      data-testid="{testid}-input"
    ></ui-text-field>
    <ui-button
      type="submit"
      square
      shape="circle"
      hit
      label={action}
      title={action}
      disabled={draft.trim() === '' || problem !== null}
      data-testid="{testid}-add"
    >
      <ui-icon slot="icon" name="plus"></ui-icon>
    </ui-button>
  </div>
  <!-- said where the mistake was made, and gone the moment the name is right -->
  {#if problem}
    <p class="problem" data-testid="{testid}-problem">{problem}</p>
  {/if}
</form>

<style>
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  .problem {
    margin: 8px 4px 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
</style>
