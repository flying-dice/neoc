<!-- @organism NoteList — the notes of what is shown, under their headings (Pinned, Today, Yesterday, ...), with the search field and the button that starts a note; a folder of the person's own can be deleted from here, after an alert asks -->
<script lang="ts">
  import type { Folder, Note } from 'app:rpc'
  import ConfirmAlert from '../molecules/ConfirmAlert.svelte'
  import { stamp } from '../lib/dates'
  import { NO_PREVIEW, UNTITLED, type Section } from '../lib/views'

  interface Props {
    /** The name of what is shown. */
    title: string
    /** The notes under their headings. */
    sections: readonly Section[]
    /** The moment "today" is measured from. */
    now: Date
    /** The id of the note that is open; none is marked when left out (a narrow window showing only the list). */
    selected?: number
    /** What is searched for. */
    query: string
    /** The folder itself when one is shown and it can be deleted. */
    folder?: Folder
    /** Whether a note can be started here. */
    creatable?: boolean
    /** Show the way back to the folders (a window too narrow for both). */
    back?: boolean
    onopen: (note: Note) => void
    onsearch: (query: string) => void
    oncreate: () => void
    onremovefolder?: (folder: Folder) => void
    onback?: () => void
  }

  let {
    title,
    sections,
    now,
    selected,
    query,
    folder,
    creatable = true,
    back = false,
    onopen,
    onsearch,
    oncreate,
    onremovefolder,
    onback
  }: Props = $props()

  const count = $derived(sections.reduce((sum, s) => sum + s.notes.length, 0))
  const searching = $derived(query.trim() !== '')

  // deleting a folder takes its notes with it: the id of the folder the person has been asked about
  let asking = $state<number | null>(null)
  const confirming = $derived(folder !== undefined && asking === folder.id)

  const about = (note: Note): string => `${stamp(note.edited, now)}  ${note.preview === '' ? NO_PREVIEW : note.preview}`
</script>

<section class="list" aria-label={title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the folders" onclick={onback} data-testid="notes-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Folders
      </ui-button>
    {/if}
    <h1 data-testid="notes-title">{searching ? 'Search' : title}</h1>
    {#if folder && !searching}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        size="small"
        destructive
        hit
        disabled={confirming}
        onclick={() => (asking = folder.id)}
        data-testid="notes-delete-folder"
      >
        Delete Folder
      </ui-button>
    {/if}
    {#if creatable}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" square hit label="New Note" onclick={oncreate} data-testid="notes-new">
        <ui-icon slot="icon" name="square.and.pencil"></ui-icon>
      </ui-button>
    {/if}
  </header>
  {#if folder && confirming}
    <ConfirmAlert
      heading="Delete {folder.name}?"
      message="Its notes go to Recently Deleted."
      testid="notes-confirm-folder"
      oncancel={() => (asking = null)}
      onconfirm={() => {
        asking = null
        onremovefolder?.(folder)
      }}
    />
  {/if}
  <div class="search">
    <ui-text-field
      type="search"
      label="Search"
      placeholder="Search"
      autocomplete="off"
      enterkeyhint="search"
      maxlength={100}
      hit
      value={query}
      oninput={(e) => onsearch((e.currentTarget as unknown as { value: string }).value)}
      data-testid="notes-search"
    ></ui-text-field>
  </div>
  <div class="scroll">
    <!-- an empty selectable list is not valid ARIA: the list is there once there is a note -->
    {#if count > 0}
      <ui-list variant="plain" selection="single" label={searching ? 'Notes found' : title} data-testid="notes-items">
        {#each sections as section (section.name)}
          <ui-section header={section.name} data-testid="notes-section-{section.name}">
            {#each section.notes as note (note.id)}
              <ui-row
                heading={note.title === '' ? UNTITLED : note.title}
                subheading={about(note)}
                selected={selected === note.id}
                onpress={() => onopen(note)}
                data-testid="notes-item-{note.id}"
              ></ui-row>
            {/each}
          </ui-section>
        {/each}
      </ui-list>
    {:else if searching}
      <ui-empty-state icon="magnifyingglass" heading="No Results" message="No note says that." data-testid="notes-empty"></ui-empty-state>
    {:else}
      <ui-empty-state
        icon="square.and.pencil"
        heading="No Notes"
        message={creatable ? 'Start one with the pencil above.' : 'Notes you delete are kept here for 30 days.'}
        data-testid="notes-empty"
      ></ui-empty-state>
    {/if}
  </div>
  <p class="count" data-testid="notes-count">{count} {count === 1 ? 'Note' : 'Notes'}</p>
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
    padding: 0 8px 0 16px;
  }
  h1 {
    margin: 0 auto 0 0;
    min-width: 0;
    font: var(--type-title2);
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .search {
    padding: 4px 12px 8px;
  }
  ui-text-field {
    display: block;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
  .count {
    margin: 0;
    padding: 8px 16px 10px;
    border-top: 1px solid var(--separator);
    text-align: center;
    font: var(--type-footnote);
    color: var(--label-secondary);
    font-variant-numeric: tabular-nums;
  }
</style>
