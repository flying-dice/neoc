<!-- @molecule ResourceRow — one image, volume or network in a list: its name, a line or two about it, which containers use it, and the buttons that act on it -->
<script module lang="ts">
  /** A button of the row. */
  export interface RowAction {
    /** What it does, as a screen reader says it: `Remove nginx:1.27`. */
    label: string
    icon: string
    testid: string
    destructive?: boolean
    disabled?: boolean
    onclick: () => void
  }
</script>

<script lang="ts">
  interface Props {
    name: string
    /** Lines about it, under its name. */
    lines: readonly string[]
    /** Which containers use it, in words. */
    note: string
    /** Nothing uses it: the note is muted. */
    unused?: boolean
    actions?: readonly RowAction[]
    /** The row's test id; its note's follows from it (`-note`). */
    testid: string
  }

  let { name, lines, note, unused = false, actions = [], testid }: Props = $props()
</script>

<li class="row" data-testid={testid}>
  <div class="text">
    <span class="name">{name}</span>
    {#each lines as line, i (i)}
      <span class="about">{line}</span>
    {/each}
    <span class="note" class:unused data-testid="{testid}-note">{note}</span>
  </div>
  {#if actions.length > 0}
    <span class="acts">
      {#each actions as action (action.testid)}
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button
          variant="plain"
          square
          hit
          destructive={action.destructive ?? false}
          disabled={action.disabled ?? false}
          label={action.label}
          onclick={action.onclick}
          data-testid={action.testid}
        >
          <ui-icon slot="icon" name={action.icon}></ui-icon>
        </ui-button>
      {/each}
    </span>
  {/if}
</li>

<style>
  .row {
    list-style: none;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 56px;
    padding: 6px 4px;
    border-bottom: 1px solid var(--separator);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name,
  .about,
  .note {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font: var(--type-body);
    font-weight: 600;
  }
  .about {
    font: var(--type-footnote);
    font-family: var(--font-mono);
    color: var(--label-secondary);
  }
  .note {
    font: var(--type-footnote);
    color: var(--label);
  }
  .note.unused {
    color: var(--label-secondary);
  }
  .acts {
    flex: none;
    display: flex;
    align-items: center;
  }
</style>
