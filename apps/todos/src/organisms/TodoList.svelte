<!-- @organism TodoList — one list: its name, its todos, and the field the next one is typed in; a list of the person's own can be deleted from here, after an alert asks -->
<script lang="ts">
  type List = VosApp.Todos.List
  type Todo = VosApp.Todos.Todo
  import Composer from '../molecules/Composer.svelte'
  import TodoRow from '../molecules/TodoRow.svelte'

  interface Props {
    /** The name of the list. */
    title: string
    todos: readonly Todo[]
    /** Today, as `2026-06-09`. */
    today: string
    /** The list itself when it is one of the person's own: it has a colour, and it can be deleted. */
    list?: List
    /** Whether it can be deleted: the last list cannot. */
    removable?: boolean
    /** The name of the list a todo is in, for a list that gathers todos from several; left out, none is shown. */
    listName?: (todo: Todo) => string
    /** The id of the todo whose details are open. */
    expanded?: number | null
    /** What is typed as the next todo. */
    draft: string
    /** Whether a todo can be added here. */
    addable?: boolean
    /** Show the way back to the lists (a window too narrow for both). */
    back?: boolean
    ontoggle: (todo: Todo) => void
    onflag: (todo: Todo) => void
    onexpand: (todo: Todo) => void
    onrename: (todo: Todo, title: string) => void
    onnotes: (todo: Todo, notes: string) => void
    ondue: (todo: Todo, due: string) => void
    onremove: (todo: Todo) => void
    ondraft: (text: string) => void
    onadd: () => void
    onremovelist?: (list: List) => void
    onback?: () => void
  }

  let {
    title,
    todos,
    today,
    list,
    removable = false,
    listName,
    expanded = null,
    draft,
    addable = true,
    back = false,
    ontoggle,
    onflag,
    onexpand,
    onrename,
    onnotes,
    ondue,
    onremove,
    ondraft,
    onadd,
    onremovelist,
    onback
  }: Props = $props()

  // deleting a list deletes its todos: the id of the list the person has been asked about
  let asking = $state<number | null>(null)
  const confirming = $derived(list !== undefined && asking === list.id)
</script>

<section class="list" aria-label={title}>
  <header class="bar">
    <div class="side">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the lists" onclick={onback} data-testid="todos-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Lists
      </ui-button>
    {/if}
    </div>
    <div class="title">
      {#if list}<span class="dot" style:background="var(--ios-{list.color})"></span>{/if}
      <h1 data-testid="todos-title">{title}</h1>
    </div>
    <div class="side end">
    <p class="count" data-testid="todos-count">{todos.length}</p>
    {#if list && removable}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        size="small"
        destructive
        hit
        disabled={confirming}
        onclick={() => (asking = list.id)}
        data-testid="todos-delete-list"
      >
        Delete List
      </ui-button>
    {/if}
    </div>
  </header>
  {#if list && confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {list.name}?"
      message="Its todos are deleted with it."
      data-testid="todos-confirm"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremovelist?.(list)
      }}
    ></ui-alert>
  {/if}
  <div class="scroll">
    {#if todos.length > 0}
      <ul class="todos" data-testid="todos-items">
        {#each todos as todo (todo.id)}
          <TodoRow
            {todo}
            {today}
            listName={listName?.(todo)}
            expanded={expanded === todo.id}
            ontoggle={() => ontoggle(todo)}
            onflag={() => onflag(todo)}
            onexpand={() => onexpand(todo)}
            onrename={(text) => onrename(todo, text)}
            onnotes={(notes) => onnotes(todo, notes)}
            ondue={(due) => ondue(todo, due)}
            onremove={() => onremove(todo)}
          />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="checkmark"
        heading="No Todos"
        message={addable ? 'Type one below.' : 'Nothing has been completed yet.'}
        data-testid="todos-empty"
      ></ui-empty-state>
    {/if}
  </div>
  {#if addable}
    <div class="new">
      <Composer {draft} label="New Todo" action="Add the todo" testid="todos-new" {ondraft} {onadd} />
    </div>
  {/if}
</section>

<style>
  .list {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 8px;
  }
  .title {
    position: relative;
    min-width: 0;
  }
  /* the list's colour sits beside its name without moving the name off the bar's centre */
  .dot {
    position: absolute;
    inset-inline-end: 100%;
    margin-inline-end: 8px;
    top: 50%;
    translate: 0 -50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  /* the title is centred between a leading and a trailing group of equal share */
  .side {
    flex: 1 1 0;
    min-width: min-content;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .end {
    justify-content: flex-end;
  }
  h1 {
    margin: 0;
    min-width: 0;
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    margin: 0;
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
    color: var(--label-secondary);
    font-variant-numeric: tabular-nums;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 16px;
  }
  .todos {
    margin: 0;
    padding: 0;
  }
  .new {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--separator);
  }
</style>
