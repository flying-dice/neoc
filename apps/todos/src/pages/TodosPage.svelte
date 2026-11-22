<!-- @page TodosPage — the whole app: the lists beside the todos of the one that is shown, or one of the two in a narrow window; driven by one window's Todos state -->
<script lang="ts">
  import ListSidebar from '../organisms/ListSidebar.svelte'
  import TodoList from '../organisms/TodoList.svelte'
  import type { Todos } from '../lib/todos.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    todos: Todos
  }

  let { todos }: Props = $props()

  // below 640 wide the lists and the todos are one column each: `browsing` decides which one shows
  let width = $state(860)
  const narrow = $derived(width < 640)
  // where several lists' todos are together, each says which list it is in
  const gathered = $derived(todos.view.kind === 'smart' && todos.lists.length > 1)

  $effect(() => {
    void todos.load()
    return todos.listen()
  })
</script>

<main class="todos" bind:clientWidth={width} data-testid="todos-app">
  <div class="columns" class:narrow class:browsing={todos.browsing}>
    <div class="side">
      <ListSidebar
        lists={todos.lists}
        counts={todos.counts}
        remaining={todos.remaining}
        selected={narrow ? undefined : todos.view}
        draft={todos.listDraft}
        onselect={(view) => todos.select(view)}
        ondraft={(name) => (todos.listDraft = name)}
        onadd={() => void todos.addList()}
      />
    </div>
    <div class="detail">
      <TodoList
        title={todos.title}
        todos={todos.todos}
        today={todos.today}
        list={todos.list}
        removable={todos.lists.length > 1}
        listName={gathered ? (todo) => todos.listName(todo) : undefined}
        expanded={todos.expanded}
        draft={todos.draft}
        addable={todos.addable}
        back={narrow}
        ontoggle={(todo) => void todos.toggle(todo)}
        onflag={(todo) => void todos.flag(todo)}
        onexpand={(todo) => todos.expand(todo)}
        onrename={(todo, title) => void todos.rename(todo, title)}
        onnotes={(todo, notes) => void todos.annotate(todo, notes)}
        ondue={(todo, due) => void todos.schedule(todo, due)}
        onremove={(todo) => void todos.remove(todo)}
        ondraft={(text) => (todos.draft = text)}
        onadd={() => void todos.add()}
        onremovelist={(list) => void todos.removeList(list)}
        onback={() => todos.back()}
      />
    </div>
  </div>
  <!-- why the last call failed (the backend's own sentence, or the host's), whichever column is in front -->
  {#if todos.failure}
    <ui-banner tone="error" data-testid="todos-failure">{todos.failure}</ui-banner>
  {/if}
</main>

<style>
  .todos {
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
    grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
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
  /* one column: the lists, or the todos once a list is picked */
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
