<!-- @page NotesPage — the whole app: the folders, the notes of the one that is shown, and the note that is open, side by side; in a narrow window one of the three at a time; driven by one window's Notes state -->
<script lang="ts">
  import FolderSidebar from '../organisms/FolderSidebar.svelte'
  import NoteEditor from '../organisms/NoteEditor.svelte'
  import NoteList from '../organisms/NoteList.svelte'
  import type { Notes } from '../lib/notes.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    notes: Notes
    /** How long after the last key what is typed is saved, in milliseconds. */
    saveAfter?: number
  }

  let { notes, saveAfter = 600 }: Props = $props()

  // below 760 wide the folders, the notes and the note are one column each: `pane` decides which one shows
  let width = $state(960)
  const narrow = $derived(width < 760)

  // what is typed is saved a moment after the last key, when the text is left, and when the window goes
  let pending: ReturnType<typeof setTimeout> | undefined
  function typed(text: string): void {
    notes.text = text
    clearTimeout(pending)
    pending = setTimeout(() => void notes.save(), saveAfter)
  }
  function commit(): void {
    clearTimeout(pending)
    void notes.save()
  }

  $effect(() => {
    void notes.load()
    const stop = notes.listen()
    return () => {
      stop()
      commit()
    }
  })
</script>

<main class="notes" bind:clientWidth={width} data-testid="notes-app">
  <div class="columns" class:narrow data-pane={notes.pane}>
    <div class="side">
      <FolderSidebar
        folders={notes.folders}
        counts={notes.counts}
        selected={narrow || notes.searching ? undefined : notes.view}
        draft={notes.folderDraft}
        problem={notes.folderProblem}
        onselect={(view) => void notes.select(view)}
        ondraft={(name) => (notes.folderDraft = name)}
        onadd={() => void notes.addFolder()}
      />
    </div>
    <div class="middle">
      <NoteList
        title={notes.title}
        sections={notes.sections}
        now={notes.now}
        selected={narrow ? undefined : notes.note?.id}
        query={notes.query}
        folder={notes.removable ? notes.folder : undefined}
        creatable={notes.creatable}
        back={narrow}
        onopen={(note) => void notes.open(note)}
        onsearch={(query) => void notes.search(query)}
        oncreate={() => void notes.create()}
        onremovefolder={(folder) => void notes.removeFolder(folder)}
        onback={() => void notes.back()}
      />
    </div>
    <div class="detail">
      <NoteEditor
        note={notes.note}
        text={notes.text}
        loaded={notes.loaded}
        folders={notes.folders}
        back={narrow}
        onedit={typed}
        oncommit={commit}
        onpin={(note) => void notes.pin(note)}
        onmove={(note, folder) => void notes.move(note, folder)}
        onremove={(note) => void notes.remove(note)}
        onrestore={(note) => void notes.restore(note)}
        onpurge={(note) => void notes.purge(note)}
        onback={() => void notes.back()}
      />
    </div>
  </div>
  <!-- why the last call failed (the backend's own sentence, or the host's), whichever column is in front -->
  {#if notes.failure}
    <ui-banner tone="error" data-testid="notes-failure">{notes.failure}</ui-banner>
  {/if}
</main>

<style>
  .notes {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .columns {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(200px, 240px) minmax(240px, 300px) minmax(0, 1fr);
  }
  .side,
  .middle,
  .detail {
    min-width: 0;
    min-height: 0;
  }
  .side,
  .middle {
    border-inline-end: 1px solid var(--separator);
  }
  /* one column: the folders, the notes of one, or a note */
  .narrow {
    grid-template-columns: minmax(0, 1fr);
  }
  .narrow > div {
    display: none;
    border-inline-end: 0;
  }
  .narrow[data-pane='folders'] .side,
  .narrow[data-pane='list'] .middle,
  .narrow[data-pane='note'] .detail {
    display: block;
  }
</style>
