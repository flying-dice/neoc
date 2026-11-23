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

  // in a window too narrow for both, the split view shows one column: the kit says when, the same for every app
  let narrow = $state(false)
  // the pane in front, as the split view names its three columns
  const column = $derived(notes.pane === 'folders' ? 'sidebar' : notes.pane === 'list' ? 'content' : 'detail')

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

<main class="notes" data-testid="notes-app">
  <ui-split-view {column} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
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
    <div slot="content">
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
    <div>
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
  </ui-split-view>
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
  /* three columns want room: the folders are given less than a sidebar's usual width */
  ui-split-view {
    flex: 1;
    --ui-split-sidebar-width: 240px;
  }
</style>
