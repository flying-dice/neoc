<!-- @organism NoteEditor — one note: when it was edited, its text to type in, and what can be done with it (pin, move to a folder, delete); a deleted note is only read, and can be recovered or deleted for good after an alert asks -->
<script lang="ts">
  type Folder = VosApp.Notes.Folder
  type Note = VosApp.Notes.Note
  import { moment } from '../lib/dates'

  interface Props {
    /** The note that is open; none shows that there is none. */
    note?: Note
    /** What the note says in this window. */
    text: string
    /** Whether the text has been read: until then there is nothing to type in. */
    loaded?: boolean
    /** The folders a note can be moved to. */
    folders: readonly Folder[]
    /** Show the way back to the notes (a window too narrow for both). */
    back?: boolean
    /** Something was typed. */
    onedit: (text: string) => void
    /** The text was left: what is typed should be saved now. */
    oncommit: () => void
    onpin: (note: Note) => void
    onmove: (note: Note, folder: number) => void
    onremove: (note: Note) => void
    onrestore: (note: Note) => void
    onpurge: (note: Note) => void
    onback?: () => void
  }

  let { note, text, loaded = true, folders, back = false, onedit, oncommit, onpin, onmove, onremove, onrestore, onpurge, onback }: Props =
    $props()

  // deleting for good cannot be undone: the id of the note the person has been asked about
  let asking = $state<number | null>(null)
  const confirming = $derived(note !== undefined && asking === note.id)
</script>

<section class="editor" aria-label="Note">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the notes" onclick={onback} data-testid="notes-back-to-list">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Notes
      </ui-button>
    {/if}
    <span class="gap"></span>
    {#if note && note.deleted}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit onclick={() => onrestore(note)} data-testid="notes-restore">Recover</ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        size="small"
        destructive
        hit
        disabled={confirming}
        onclick={() => (asking = note.id)}
        data-testid="notes-purge"
      >
        Delete
      </ui-button>
    {:else if note}
      {#if folders.length > 1}
        <div class="move">
          <span aria-hidden="true">Folder</span>
          <ui-picker
            label="Folder"
            value={String(note.folder)}
            options={folders.map((folder) => ({ value: String(folder.id), label: folder.name }))}
            onchange={(e) => onmove(note, Number(e.detail.value))}
            data-testid="notes-move"
          ></ui-picker>
        </div>
      {/if}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={note.pinned ? 'tinted' : 'plain'}
        size="small"
        toggle
        pressed={note.pinned}
        hit
        onclick={() => onpin(note)}
        data-testid="notes-pin"
      >
        {note.pinned ? 'Pinned' : 'Pin'}
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" square destructive hit label="Delete the note" onclick={() => onremove(note)} data-testid="notes-remove">
        <ui-icon slot="icon" name="trash"></ui-icon>
      </ui-button>
    {/if}
  </header>
  {#if note && confirming}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete this note for good?"
      message="It cannot be brought back."
      data-testid="notes-confirm-purge"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onpurge(note)
      }}
    ></ui-alert>
  {/if}
  {#if note}
    <p class="edited" data-testid="notes-edited">{note.deleted ? 'Deleted. Edited' : 'Edited'} {moment(note.edited)}</p>
    <ui-text-area
      class="text"
      variant="bare"
      label="Note"
      placeholder={loaded && !note.deleted ? 'Start writing' : ''}
      maxlength={20000}
      readonly={note.deleted}
      disabled={!loaded}
      value={text}
      oninput={(e) => onedit((e.currentTarget as unknown as { value: string }).value)}
      onblur={oncommit}
      data-testid="notes-text"
    ></ui-text-area>
  {:else}
    <div class="none">
      <ui-empty-state icon="doc" heading="No Note Selected" message="Pick a note, or start one." data-testid="notes-none"></ui-empty-state>
    </div>
  {/if}
</section>

<style>
  .editor {
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
    padding: 0 8px 0 12px;
  }
  .gap {
    flex: 1;
  }
  .move {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  .move ui-picker {
    max-width: 160px;
  }
  .edited {
    margin: 0;
    padding: 4px 16px 0;
    text-align: center;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  /* the note is the pane: it takes the room that is left, with the pane's own margins around the words */
  .text {
    flex: 1;
  }
  .text::part(input) {
    padding: 12px 20px 20px;
  }
  .none {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
  }
</style>
