<!-- @molecule IssueCard — one issue on the board: its title, the kind of work it is, its key and its priority; a tap opens it, it can be dragged to another column, and a button moves it one column on -->
<script lang="ts">
  import type { Issue } from '@projects/rpc'
  import { priorityOf, typeOf } from '../lib/catalog'

  interface Props {
    issue: Issue
    /** The name of the column after the one it is in; left out in the last column. */
    next?: string
    onopen: () => void
    /** Move it to the next column. */
    onadvance: () => void
    /** It was picked up to be dragged, or put down. */
    ondrag: (dragging: boolean) => void
  }

  let { issue, next, onopen, onadvance, ondrag }: Props = $props()

  // an issue is known by its key (WEB-12), which never changes: its test ids are named after it
  const id = $derived(`projects-card-${issue.key}`)
  const type = $derived(typeOf(issue.type))
</script>

<!-- dragging is a shortcut for a pointer: the button on the card and the status in the issue do the same for everyone -->
<li
  class="card"
  draggable="true"
  ondragstart={(e) => {
    e.dataTransfer?.setData('text/plain', issue.key)
    ondrag(true)
  }}
  ondragend={() => ondrag(false)}
  data-testid={id}
>
  <button class="open" type="button" onclick={onopen} data-testid="{id}-open">
    <span class="title" data-testid="{id}-title">{issue.title}</span>
    <span class="about">
      <span class="type" style:color="var(--ios-{type.color})"><ui-icon name={type.icon} size={14} weight={2.5} label={type.label}></ui-icon></span>
      <span class="key" data-testid="{id}-key">{issue.key}</span>
      {#if issue.priority !== 'medium'}
        <span class="priority" class:high={issue.priority === 'high'} data-testid="{id}-priority">{priorityOf(issue.priority).label}</span>
      {/if}
    </span>
  </button>
  {#if next}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" square label="Move {issue.key} to {next}" onclick={onadvance} data-testid="{id}-advance">
      <ui-icon slot="icon" name="chevron.right"></ui-icon>
    </ui-button>
  {/if}
</li>

<style>
  .card {
    list-style: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding-inline-end: 4px;
    border-radius: var(--radius-md);
    background: var(--bg-grouped-secondary);
    box-shadow: var(--shadow-card);
  }
  .open {
    flex: 1;
    min-width: 0;
    min-height: 44px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 4px 10px 12px;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--label);
    font: var(--type-subheadline);
    text-align: start;
    cursor: pointer;
  }
  .open:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .title {
    overflow-wrap: anywhere;
  }
  .about {
    display: flex;
    align-items: center;
    gap: 6px;
    font: var(--type-caption1);
    color: var(--label-secondary);
    font-variant-numeric: tabular-nums;
  }
  .type {
    display: inline-flex;
  }
  .priority {
    margin-inline-start: auto;
  }
  .priority.high {
    color: var(--ios-red);
    font-weight: 600;
  }
</style>
