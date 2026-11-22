<!-- @page NotebooksPage — the whole app: a notebook's notes beside the one that is open or the notebook's graph, or one of the two in a narrow window, with the quick switcher over them; driven by one window's Notebooks state -->
<script lang="ts">
  import Switcher from '../molecules/Switcher.svelte'
  import GraphPane from '../organisms/GraphPane.svelte'
  import NotePane from '../organisms/NotePane.svelte'
  import Sidebar from '../organisms/Sidebar.svelte'
  import type { Notebooks } from '../lib/notebooks.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    notebooks: Notebooks
    /** Open a link out of the app (the host's `openLink`). */
    onlink: (url: string) => void
  }

  let { notebooks, onlink }: Props = $props()

  // below 700 wide the notes and the note are one column each: `browsing` decides which one shows
  let width = $state(1000)
  const narrow = $derived(width < 700)
  const note = $derived(notebooks.note)

  $effect(() => {
    void notebooks.load()
    return notebooks.listen()
  })

  // ⌘O or Ctrl+O opens a note by name; ⌘E or Ctrl+E turns between writing and reading
  function keyed(event: KeyboardEvent): void {
    if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return
    const key = event.key.toLowerCase()
    if (key === 'o' && notebooks.notebook) {
      event.preventDefault()
      notebooks.switching = true
    } else if (key === 'e' && note) {
      event.preventDefault()
      notebooks.setMode(notebooks.mode === 'edit' ? 'read' : 'edit')
    }
  }
</script>

<svelte:window onkeydown={keyed} />

<main class="notebooks" bind:clientWidth={width} data-testid="notebooks-app">
  <div class="columns" class:narrow class:browsing={notebooks.browsing}>
    <div class="side">
      <Sidebar
        notebooks={notebooks.notebooks}
        notebook={notebooks.notebook}
        notes={notebooks.notes}
        hits={notebooks.hits}
        tags={notebooks.tags}
        tag={notebooks.tag}
        query={notebooks.query}
        selected={narrow || notebooks.pane === 'graph' ? undefined : note?.id}
        graphing={!narrow && notebooks.pane === 'graph'}
        now={notebooks.now}
        draft={notebooks.notebookDraft}
        problem={notebooks.notebookProblem}
        onnotebook={(id) => void notebooks.selectNotebook(id)}
        onnew={() => void notebooks.create()}
        ondaily={() => void notebooks.daily()}
        onswitch={() => (notebooks.switching = true)}
        ongraph={() => notebooks.show(notebooks.pane === 'graph' && !narrow ? 'note' : 'graph')}
        onsearch={(query) => void notebooks.search(query)}
        ontag={(tag) => notebooks.filter(tag)}
        onopen={(id) => void notebooks.open(id)}
        ondraft={(name) => notebooks.nameNotebook(name)}
        onaddnotebook={() => notebooks.addNotebook()}
        onremovenotebook={() => void notebooks.removeNotebook()}
      />
    </div>
    <div class="detail">
      {#if notebooks.pane === 'graph'}
        <GraphPane
          title={notebooks.notebook?.name ?? ''}
          nodes={notebooks.graph.nodes}
          links={notebooks.graph.links}
          selected={note?.id}
          back={narrow}
          onopen={(id) => void notebooks.open(id)}
          onback={() => notebooks.back()}
        />
      {:else if note}
        <NotePane
          {note}
          body={notebooks.body}
          mode={notebooks.mode}
          status={notebooks.status}
          titleProblem={notebooks.titleProblem}
          backlinks={notebooks.backlinks}
          resolves={(title) => notebooks.resolves(title)}
          back={narrow}
          onedit={(text) => notebooks.edit(text)}
          onflush={() => void notebooks.flush()}
          onmode={(mode) => notebooks.setMode(mode)}
          onretitle={(title) => notebooks.retitle(title)}
          onrename={(title) => void notebooks.rename(title)}
          onpin={() => void notebooks.pin()}
          onremove={() => void notebooks.remove()}
          onfollow={(title) => void notebooks.follow(title)}
          {onlink}
          ontag={(tag) => notebooks.filter(tag)}
          ontick={(line) => void notebooks.tick(line)}
          onopen={(id) => void notebooks.open(id)}
          onback={() => notebooks.back()}
        />
      {:else}
        <div class="none">
          <ui-empty-state icon="doc" heading="No Note Open" message="Pick a note, or start one with New Note." data-testid="notebooks-none"></ui-empty-state>
        </div>
      {/if}
    </div>
  </div>
  <!-- why the last call failed when it was not about what was typed (the host's own sentence), whichever column is in front -->
  {#if notebooks.failure}
    <ui-banner tone="error" data-testid="notebooks-failure">{notebooks.failure}</ui-banner>
  {/if}
  {#if notebooks.switching}
    <Switcher
      notes={notebooks.all}
      testid="notebooks-switcher"
      onopen={(id) => void notebooks.open(id)}
      oncreate={(title) => void notebooks.follow(title)}
      onclose={() => (notebooks.switching = false)}
    />
  {/if}
</main>

<style>
  .notebooks {
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
    grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
  }
  .side {
    min-width: 0;
    min-height: 0;
    border-inline-end: 1px solid var(--separator);
  }
  .detail {
    min-width: 0;
    min-height: 0;
  }
  .none {
    box-sizing: border-box;
    height: 100%;
    padding-top: 28px;
    display: grid;
    place-items: center;
  }
  /* one column: the notes, or the note once one is picked */
  .narrow {
    grid-template-columns: minmax(0, 1fr);
  }
  .narrow:not(.browsing) .side,
  .narrow.browsing .detail {
    display: none;
  }
  .narrow .side {
    border-inline-end: 0;
  }
</style>
