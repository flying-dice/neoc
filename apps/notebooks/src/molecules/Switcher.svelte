<!-- @molecule Switcher — the quick switcher: a field a note's name is typed in over the window, the notes it could mean under it (the arrow keys move through them, Return opens the one marked), and a last row that makes a note of that name when there is none -->
<script lang="ts">
  type Summary = VosApp.Notebooks.Summary
  import { matching, named } from '../lib/library'

  interface Props {
    /** The notes of the notebook, as its list has them. */
    notes: readonly Summary[]
    /** The switcher's test id; the field's is this with `-input`, a note's with `-note-<id>`, the row that makes one with `-create`. */
    testid: string
    onopen: (id: number) => void
    /** Make a note with the name that was typed, and open it. */
    oncreate: (title: string) => void
    onclose: () => void
  }

  let { notes, testid, onopen, oncreate, onclose }: Props = $props()

  const SHOWN = 8

  let dialog = $state<HTMLDialogElement>()
  let field = $state<HTMLInputElement>()
  let typed = $state('')
  // which row is marked, counted through the notes and then the row that makes one
  let marked = $state(0)

  const found = $derived(matching(notes, typed).slice(0, SHOWN))
  // a name no note has can be made into one
  const creatable = $derived(typed.trim() !== '' && named(notes, notes[0]?.notebook ?? null, typed) === undefined)
  const rows = $derived(found.length + (creatable ? 1 : 0))

  $effect(() => {
    dialog?.showModal()
    field?.focus()
    return () => dialog?.close()
  })

  function choose(row: number): void {
    const note = found[row]
    if (note) onopen(note.id)
    else if (creatable && row === found.length) oncreate(typed.trim())
  }

  function keyed(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (rows > 0) marked = (marked + (event.key === 'ArrowDown' ? 1 : rows - 1)) % rows
    } else if (event.key === 'Enter') {
      event.preventDefault()
      choose(marked)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      onclose()
    }
  }
</script>

<!-- a tap on the dimmed window around the switcher closes it, as Escape does -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialog}
  class="switcher"
  aria-label="Open a note"
  onclick={(e) => {
    if (e.target === dialog) onclose()
  }}
  oncancel={(e) => {
    e.preventDefault()
    onclose()
  }}
  data-testid={testid}
>
  <div class="box">
    <!-- the dialog gives its first field the focus as it opens: `autofocus` is refused in an app's frame -->
    <input
      bind:this={field}
      type="text"
      role="combobox"
      aria-label="Note name"
      aria-expanded="true"
      aria-controls="{testid}-rows"
      aria-activedescendant={rows > 0 ? `${testid}-row-${marked}` : undefined}
      placeholder="Find or make a note…"
      autocomplete="off"
      maxlength={120}
      value={typed}
      oninput={(e) => {
        typed = e.currentTarget.value
        marked = 0
      }}
      onkeydown={keyed}
      data-testid="{testid}-input"
    />
    <ul id="{testid}-rows" role="listbox" aria-label="Notes">
      {#each found as note, row (note.id)}
        <!-- the keys are the field's: a row is pressed, or chosen from the field with Return -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{testid}-row-{row}"
          role="option"
          aria-selected={row === marked}
          class:marked={row === marked}
          onclick={() => choose(row)}
          data-testid="{testid}-note-{note.id}"
        >
          <span class="name">{note.title}</span>
          {#if note.excerpt}<span class="about">{note.excerpt}</span>{/if}
        </li>
      {/each}
      {#if creatable}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{testid}-row-{found.length}"
          role="option"
          aria-selected={marked === found.length}
          class:marked={marked === found.length}
          onclick={() => choose(found.length)}
          data-testid="{testid}-create"
        >
          <span class="name">Make “{typed.trim()}”</span>
          <span class="about">A new note</span>
        </li>
      {/if}
      {#if rows === 0}
        <li class="none" role="option" aria-selected="false" aria-disabled="true">No notes yet</li>
      {/if}
    </ul>
  </div>
</dialog>

<style>
  .switcher {
    box-sizing: border-box;
    width: min(480px, calc(100vw - 32px));
    /* one height whatever is found: the field stays where it is as the rows under it change */
    height: min(360px, calc(100vh - 64px));
    padding: 0;
    border: 0;
    border-radius: var(--radius-xl);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
  }
  .switcher::backdrop {
    background: var(--dimming);
  }
  .box {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    min-height: 48px;
    padding: 0 16px;
    border: 0;
    border-bottom: 1px solid var(--separator);
    background: none;
    color: var(--label);
    font: var(--type-body);
    outline: none;
  }
  input::placeholder {
    color: var(--label-secondary);
  }
  /* the field always has the focus while the switcher is open: the ring is on the box it fills */
  .switcher:focus-within {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  ul {
    margin: 0;
    padding: 8px;
    list-style: none;
    overflow-y: auto;
  }
  li {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-height: 44px;
    padding: 4px 12px;
    border-radius: var(--radius-md);
    cursor: pointer;
  }
  li.marked {
    background: var(--fill-secondary);
  }
  .name,
  .about {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font: var(--type-body);
  }
  .about,
  .none {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .none {
    cursor: default;
  }
</style>
