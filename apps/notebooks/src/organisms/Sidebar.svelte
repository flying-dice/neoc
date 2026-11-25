<!-- @organism Sidebar — one notebook's contents: the notebook itself (picked, added, deleted after an alert asks), what can be started from it (a note, today's note, the quick switcher, the graph), the search, the tags, and its notes or what the search found -->
<script lang="ts">
  import type { Hit, Notebook, Summary } from '@notebooks/rpc'
  import Composer from '../molecules/Composer.svelte'
  import { changedLabel } from '../lib/dates'
  import type { TagCount } from '../lib/library'

  interface Props {
    notebooks: readonly Notebook[]
    /** The notebook that is shown. */
    notebook?: Notebook
    /** Its notes, in the order they are listed. */
    notes: readonly Summary[]
    /** What the search found, in place of the notes; null while nothing is searched for. */
    hits?: readonly Hit[] | null
    /** The tags of its notes, with how many notes have each. */
    tags: readonly TagCount[]
    /** The tag the notes are narrowed to. */
    tag?: string | null
    /** The words that are searched for. */
    query: string
    /** The id of the note that is open; none is marked when left out. */
    selected?: number
    /** The graph is what is shown beside the list. */
    graphing?: boolean
    /** The moment on the person's clock: what "today" is, next to each note. */
    now: Date
    /** The name typed for a new notebook, and what is wrong with it. */
    draft: string
    problem?: string | null
    onnotebook: (id: number) => void
    onnew: () => void
    ondaily: () => void
    onswitch: () => void
    ongraph: () => void
    onsearch: (query: string) => void
    ontag: (tag: string) => void
    onopen: (id: number) => void
    ondraft: (name: string) => void
    /** Add the notebook that is named; answers whether it was added. */
    onaddnotebook: () => Promise<boolean>
    onremovenotebook: () => void
  }

  let {
    notebooks,
    notebook,
    notes,
    hits = null,
    tags,
    tag = null,
    query,
    selected,
    graphing = false,
    now,
    draft,
    problem = null,
    onnotebook,
    onnew,
    ondaily,
    onswitch,
    ongraph,
    onsearch,
    ontag,
    onopen,
    ondraft,
    onaddnotebook,
    onremovenotebook
  }: Props = $props()

  // the field a new notebook is named in is there once it is asked for
  let adding = $state(false)
  // deleting a notebook deletes its notes: the person is being asked
  let asking = $state(false)

  async function add(): Promise<void> {
    if (await onaddnotebook()) adding = false
  }
</script>

<nav class="sidebar" aria-label="Notebook">
  <div class="top">
    <div class="notebook">
      <ui-picker
        label="Notebook"
        value={notebook ? String(notebook.id) : ''}
        options={notebooks.map((each) => ({ value: String(each.id), label: each.name }))}
        onchange={(e) => onnotebook(Number(e.detail.value))}
        data-testid="notebooks-notebook"
      ></ui-picker>
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={adding ? 'tinted' : 'plain'}
        square
        toggle
        pressed={adding}
        label="New notebook"
        onclick={() => (adding = !adding)}
        data-testid="notebooks-add-notebook"
      >
        <ui-icon slot="icon" name="plus"></ui-icon>
      </ui-button>
      {#if notebook && notebooks.length > 1}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button
          variant="plain"
          square
          destructive
          label="Delete {notebook.name}"
          disabled={asking}
          onclick={() => (asking = true)}
          data-testid="notebooks-delete-notebook"
        >
          <ui-icon slot="icon" name="trash"></ui-icon>
        </ui-button>
      {/if}
    </div>
    {#if adding}
      <Composer {draft} label="New Notebook" action="Add the notebook" testid="notebooks-new-notebook" {problem} {ondraft} onadd={() => void add()} />
    {/if}
    <div class="actions">
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button class="new" variant="gray" disabled={!notebook} onclick={onnew} data-testid="notebooks-new">
        <ui-icon slot="icon" name="square.and.pencil"></ui-icon>
        New Note
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="gray" square label="Today's note" disabled={!notebook} onclick={ondaily} data-testid="notebooks-daily">
        <ui-icon slot="icon" name="sun"></ui-icon>
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="gray" square label="Open a note by name" disabled={!notebook} onclick={onswitch} data-testid="notebooks-switch">
        <ui-icon slot="icon" name="bolt"></ui-icon>
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={graphing ? 'tinted' : 'gray'}
        square
        toggle
        pressed={graphing}
        label="Graph"
        disabled={!notebook}
        onclick={ongraph}
        data-testid="notebooks-graph-toggle"
      >
        <ui-icon slot="icon" name="network"></ui-icon>
      </ui-button>
    </div>
    <ui-text-field
      type="search"
      label="Search the notes"
      placeholder="Search"
      autocomplete="off"
      enterkeyhint="search"
      maxlength={200}
      value={query}
      oninput={(e) => onsearch((e.currentTarget as unknown as { value: string }).value)}
      data-testid="notebooks-search"
    ></ui-text-field>
    {#if tags.length > 0 && hits === null}
      <div class="tags" role="group" aria-label="Tags" data-testid="notebooks-tags">
        {#each tags as each (each.tag)}
          <button
            type="button"
            class="tag"
            aria-pressed={tag === each.tag}
            onclick={() => ontag(each.tag)}
            data-testid="notebooks-tag-{each.tag}"
          >
            #{each.tag} <span class="count">{each.count}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <div class="notes">
    {#if hits !== null}
      {#if hits.length > 0}
        <ui-list variant="sidebar" selection="single" label="Found" data-testid="notebooks-hits">
          <ui-section header="Found">
            {#each hits as hit (hit.id)}
              <ui-row
                heading={hit.title}
                subheading={hit.excerpt}
                icon="magnifyingglass"
                icon-color="gray"
                selected={selected === hit.id}
                onpress={() => onopen(hit.id)}
                data-testid="notebooks-hit-{hit.id}"
              ></ui-row>
            {/each}
          </ui-section>
        </ui-list>
      {:else}
        <ui-empty-state icon="magnifyingglass" heading="Nothing Found" message="No note of this notebook has those words." data-testid="notebooks-no-hits"
        ></ui-empty-state>
      {/if}
    {:else if notes.length > 0}
      <!-- a row's own name leaves its value out: each is named with all it shows -->
      <ui-list variant="sidebar" selection="single" label="Notes" data-testid="notebooks-notes">
        <ui-section header={tag === null ? 'Notes' : `#${tag}`}>
          {#each notes as note (note.id)}
            {@const changed = changedLabel(note.updated, now)}
            <ui-row
              heading={note.title}
              subheading={note.excerpt}
              value={changed}
              label={[note.title, note.excerpt, changed].filter(Boolean).join(', ')}
              icon={note.pinned ? 'star' : 'doc'}
              icon-color={note.pinned ? 'yellow' : 'purple'}
              selected={selected === note.id}
              onpress={() => onopen(note.id)}
              data-testid="notebooks-note-{note.id}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state icon="doc" heading="No Notes" message="Start one with New Note." data-testid="notebooks-empty"></ui-empty-state>
    {/if}
  </div>
  {#if notebook && asking}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {notebook.name}?"
      message="Its notes are deleted with it."
      data-testid="notebooks-confirm-notebook"
      oncancel={() => (asking = false)}
      onconfirm={() => {
        asking = false
        onremovenotebook()
      }}
    ></ui-alert>
  {/if}
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    background: var(--bg-secondary);
  }
  .top {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 12px 8px;
  }
  .notebook,
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  /* the button that starts a note takes the room the three beside it leave */
  .actions :global(.new) {
    flex: 1;
    min-width: 0;
  }
  .actions :global(.new::part(button)) {
    width: 100%;
  }
  ui-picker {
    flex: 1;
  }
  /* the notebook is the sidebar's heading */
  ui-picker::part(select) {
    font: var(--type-headline);
  }
  .tag:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-height: 72px;
    overflow-y: auto;
    /* room for the focus ring of the tags at the edges */
    padding: 3px;
    margin: -3px;
  }
  .tag {
    display: inline-block;
    line-height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--fill-tertiary);
    color: var(--label);
    font: var(--type-subheadline);
    cursor: pointer;
  }
  .tag[aria-pressed='true'] {
    background: var(--tint-fill);
    color: #fff;
  }
  .count {
    color: var(--label-secondary);
    font-variant-numeric: tabular-nums;
  }
  .tag[aria-pressed='true'] .count {
    color: inherit;
  }
  .notes {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 8px;
  }
</style>
