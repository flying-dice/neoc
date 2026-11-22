<!-- @molecule TodoRow — one todo: the check that ticks it off, its title (typed over to rename it), when it is due, its flag, and its details (notes, the day, delete) when they are open -->
<script lang="ts">
  import type { Todo } from 'app:rpc'
  import { dueLabel, isOverdue } from '../lib/dates'

  interface Props {
    todo: Todo
    /** Today, as `2026-06-09`: what the due day is read against. */
    today: string
    /** The name of the list it is in, shown where todos of several lists are together. */
    listName?: string
    /** The details are open. */
    expanded?: boolean
    ontoggle: () => void
    onflag: () => void
    onexpand: () => void
    onrename: (title: string) => void
    onnotes: (notes: string) => void
    /** A day as `2026-06-09`, or empty for none. */
    ondue: (due: string) => void
    onremove: () => void
  }

  let { todo, today, listName = '', expanded = false, ontoggle, onflag, onexpand, onrename, onnotes, ondue, onremove }: Props =
    $props()

  const id = $derived(`todos-item-${todo.id}`)
  const due = $derived(dueLabel(todo.due, today))
  const overdue = $derived(!todo.done && isOverdue(todo.due, today))

  /** A kit field: the text it holds. */
  type Field = HTMLElement & { value: string }

  // A field commits when it is left, and on Enter (its form is submitted). A title cannot be blank: the field takes back
  // the one the todo has. Text the todo already has goes nowhere.
  function rename(field: Field): void {
    if (field.value.trim() === '') field.value = todo.title
    else if (field.value.trim() !== todo.title) onrename(field.value)
  }
  function annotate(field: Field): void {
    if (field.value !== todo.notes) onnotes(field.value)
  }
  /** Enter in a field: commit what it holds, and stay in it. */
  function entered(event: SubmitEvent, commit: (field: Field) => void): void {
    event.preventDefault()
    const field = (event.currentTarget as HTMLFormElement).querySelector<Field>('ui-text-field')
    if (field) commit(field)
  }
</script>

<li class="todo" class:done={todo.done} data-testid={id}>
  <div class="line">
    <ui-check
      hit
      checked={todo.done}
      label="{todo.done ? 'Completed' : 'Complete'}: {todo.title}"
      onchange={ontoggle}
      data-testid="{id}-check"
    ></ui-check>
    <div class="text">
      <form onsubmit={(e) => entered(e, rename)}>
        <ui-text-field
          variant="bare"
          label="Title"
          autocomplete="off"
          enterkeyhint="done"
          maxlength={500}
          value={todo.title}
          onchange={(e) => rename(e.currentTarget as Field)}
          data-testid="{id}-title"
        ></ui-text-field>
      </form>
      {#if listName || due || (todo.notes && !expanded)}
        <p class="about">
          {#if listName}<span data-testid="{id}-list">{listName}</span>{/if}
          {#if due}<span class:overdue data-testid="{id}-due-label">{due}</span>{/if}
          {#if todo.notes && !expanded}<span class="notes" data-testid="{id}-notes-label">{todo.notes}</span>{/if}
        </p>
      {/if}
    </div>
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant={todo.flagged ? 'tinted' : 'plain'}
      tint={todo.flagged ? 'var(--ios-orange)' : 'var(--label-secondary)'}
      square
      toggle
      pressed={todo.flagged}
      label="Flag: {todo.title}"
      onclick={onflag}
      data-testid="{id}-flag"
    >
      <ui-icon slot="icon" name="flag"></ui-icon>
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant={expanded ? 'tinted' : 'plain'}
      square
      toggle
      pressed={expanded}
      label="Details: {todo.title}"
      onclick={onexpand}
      data-testid="{id}-details"
    >
      <ui-icon slot="icon" name="ellipsis"></ui-icon>
    </ui-button>
  </div>
  {#if expanded}
    <div class="details">
      <form onsubmit={(e) => entered(e, annotate)}>
        <ui-text-field
          label="Notes"
          placeholder="Notes"
          autocomplete="off"
          enterkeyhint="done"
          maxlength={2000}
          value={todo.notes}
          onchange={(e) => annotate(e.currentTarget as Field)}
          data-testid="{id}-notes"
        ></ui-text-field>
      </form>
      <div class="when">
        <label>
          <span>Due</span>
          <input type="date" value={todo.due} onchange={(e) => ondue(e.currentTarget.value)} data-testid="{id}-due" />
        </label>
        {#if todo.due}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <ui-button variant="plain" size="small" onclick={() => ondue('')} data-testid="{id}-no-due">No day</ui-button>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button class="remove" variant="plain" size="small" destructive onclick={onremove} data-testid="{id}-remove">
          <ui-icon slot="icon" name="trash"></ui-icon>
          Delete
        </ui-button>
      </div>
    </div>
  {/if}
</li>

<style>
  .todo {
    list-style: none;
    padding: 4px 0;
    border-bottom: 1px solid var(--separator);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 44px;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  ui-text-field {
    display: block;
  }
  .done ui-text-field[variant='bare']::part(input) {
    color: var(--label-secondary);
  }
  .about {
    display: flex;
    gap: 8px;
    margin: 0 0 4px;
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
  }
  .about span {
    flex: none;
  }
  .about .overdue {
    color: var(--ios-red);
  }
  /* the notes take what room is left, and are cut short */
  .about .notes {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* under the title, clear of the check */
  .details {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 0 8px 48px;
  }
  .when {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .when label {
    display: flex;
    align-items: center;
    gap: 8px;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  .when input {
    box-sizing: border-box;
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--fill-tertiary);
    color: var(--label);
    font: var(--type-body);
  }
  .when input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .when :global(.remove) {
    margin-inline-start: auto;
  }
</style>
