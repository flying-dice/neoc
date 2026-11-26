<!-- @organism NotePane — one note: its title (typed over to rename it, with what is wrong with a title said under it), whether it is written or read, its bookmark, the way to delete it after an alert asks, its text as Markdown to write or as it reads, and the notes that link to it -->
<script lang="ts">
  type Backlink = VosApp.Notebooks.Backlink
  type Note = VosApp.Notebooks.Note
  import Markdown from '../molecules/Markdown.svelte'
  import type { Mode, Status } from '../lib/notebooks.svelte'

  interface Props {
    note: Note
    /** Its text as it is being written. */
    body: string
    mode: Mode
    /** Whether the backend has what is written. */
    status: Status
    /** What is wrong with the title that was typed. */
    titleProblem?: string | null
    backlinks: readonly Backlink[]
    /** Whether a link's title names a note. */
    resolves?: (title: string) => boolean
    /** Show the way back to the list (a window too narrow for both). */
    back?: boolean
    onedit: (text: string) => void
    /** The writing was left: send it now. */
    onflush: () => void
    onmode: (mode: Mode) => void
    /** A title is being typed. */
    onretitle: (title: string) => void
    /** A title was committed. */
    onrename: (title: string) => void
    onpin: () => void
    onremove: () => void
    /** A link to a note was pressed: its title. */
    onfollow: (title: string) => void
    /** A link out of the app was pressed. */
    onlink: (url: string) => void
    ontag: (tag: string) => void
    /** A task was ticked: its line. */
    ontick: (line: number) => void
    /** A note that links here was pressed. */
    onopen: (id: number) => void
    onback?: () => void
  }

  let {
    note,
    body,
    mode,
    status,
    titleProblem = null,
    backlinks,
    resolves,
    back = false,
    onedit,
    onflush,
    onmode,
    onretitle,
    onrename,
    onpin,
    onremove,
    onfollow,
    onlink,
    ontag,
    ontick,
    onopen,
    onback
  }: Props = $props()

  const MODES = [
    { value: 'edit', label: 'Write' },
    { value: 'read', label: 'Read' }
  ]
  const STATUS: Record<Status, string> = { saved: 'Saved', saving: 'Saving…', edited: 'Edited' }

  /** A kit field: the text it holds. */
  type Field = HTMLElement & { value: string }

  // the id of the note the person has been asked about: another note opened under the alert is not the one it asked for
  let asking = $state<number | null>(null)
  const confirming = $derived(asking === note.id)

  /** Enter in the title: commit it, and stay in the field. */
  function entered(event: SubmitEvent): void {
    event.preventDefault()
    const field = (event.currentTarget as HTMLFormElement).querySelector<Field>('ui-text-field')
    if (field) onrename(field.value)
  }
</script>

<section class="pane" aria-label={note.title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the notes" onclick={onback} data-testid="notebooks-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Notes
      </ui-button>
    {/if}
    <p class="status" role="status" data-testid="notebooks-status">{STATUS[status]}</p>
    <ui-segmented
      label="How the note is shown"
      options={MODES}
      value={mode}
      onchange={(e) => onmode((e.currentTarget as Field).value === 'read' ? 'read' : 'edit')}
      data-testid="notebooks-mode"
    ></ui-segmented>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant={note.pinned ? 'tinted' : 'plain'}
      tint={note.pinned ? 'var(--ios-orange)' : 'var(--label-secondary)'}
      square
      toggle
      pressed={note.pinned}
      label="Bookmark"
      onclick={onpin}
      data-testid="notebooks-pin"
    >
      <ui-icon slot="icon" name="star"></ui-icon>
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" square destructive label="Delete the note" disabled={confirming} onclick={() => (asking = note.id)} data-testid="notebooks-delete">
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
  </header>
  <div class="title">
    <form onsubmit={entered}>
      {#key note.id}
        <ui-text-field
          variant="bare"
          label="Title"
          autocomplete="off"
          enterkeyhint="done"
          maxlength={120}
          invalid={titleProblem !== null}
          description={titleProblem ?? undefined}
          value={note.title}
          oninput={(e) => onretitle((e.currentTarget as unknown as Field).value)}
          onchange={(e) => onrename((e.currentTarget as Field).value)}
          data-testid="notebooks-title"
        ></ui-text-field>
      {/key}
    </form>
    {#if titleProblem !== null}
      <p class="problem" data-testid="notebooks-title-problem">{titleProblem}</p>
    {/if}
  </div>
  <div class="content" class:writing={mode === 'edit'}>
    {#if mode === 'edit'}
      <ui-text-area
        label="The note, in Markdown"
        placeholder={'Write in Markdown. [[Another note]] links to it, #tag tags this one.'}
        maxlength={200000}
        value={body}
        oninput={(e) => onedit((e.currentTarget as unknown as { value: string }).value)}
        onblur={onflush}
        data-testid="notebooks-editor"
      ></ui-text-area>
    {:else if body.trim() === ''}
      <ui-empty-state icon="doc" heading="Nothing Written" message="Choose Write to start the note." data-testid="notebooks-blank"></ui-empty-state>
    {:else}
      <Markdown {body} testid="notebooks-reading" {resolves} {onfollow} {onlink} {ontag} {ontick} />
    {/if}
  </div>
  {#if backlinks.length > 0}
    <aside class="backlinks" aria-label="Linked from" data-testid="notebooks-backlinks">
      <h2>Linked from</h2>
      <ul>
        {#each backlinks as from (from.id)}
          <li>
            <button type="button" onclick={() => onopen(from.id)} data-testid="notebooks-backlink-{from.id}">
              <span class="name">{from.title}</span>
              {#if from.excerpt}<span class="about">{from.excerpt}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    </aside>
  {/if}
  {#if confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {note.title}?"
      message="The links to it stay, and lead to a new note."
      data-testid="notebooks-confirm"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremove()
      }}
    ></ui-alert>
  {/if}
</section>

<style>
  .pane {
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
    padding: 0 16px;
  }
  .status {
    margin: 0 auto 0 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
  }
  .title {
    padding: 0 16px 4px;
  }
  .title ui-text-field {
    display: block;
  }
  .title ui-text-field::part(input) {
    font: var(--type-title1);
    font-weight: 700;
  }
  .problem {
    margin: 2px 0 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 16px 16px;
  }
  /* the writing fills the pane and scrolls in itself */
  .content.writing {
    display: flex;
    overflow: hidden;
    padding-bottom: 12px;
  }
  ui-text-area {
    flex: 1;
  }
  /* Markdown is written in the fixed-width face, so that its marks line up */
  ui-text-area::part(input) {
    font-family: var(--font-mono);
    font-size: 14px;
    line-height: 1.55;
  }
  .backlinks button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .backlinks {
    flex: none;
    max-height: 30%;
    overflow-y: auto;
    padding: 8px 16px 12px;
    border-top: 1px solid var(--separator);
  }
  h2 {
    margin: 0 0 4px;
    font: var(--type-footnote);
    font-weight: 600;
    text-transform: uppercase;
    color: var(--label-secondary);
  }
  .backlinks ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .backlinks button {
    box-sizing: border-box;
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    min-height: 32px;
    padding: 4px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--label);
    font: var(--type-subheadline);
    text-align: start;
    cursor: pointer;
  }
  .backlinks button:hover {
    background: var(--fill-quaternary);
  }
  .name {
    flex: none;
    max-width: 50%;
    color: var(--link);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .about {
    min-width: 0;
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
