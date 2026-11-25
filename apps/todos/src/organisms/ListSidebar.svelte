<!-- @organism ListSidebar — the smart lists and the person's own, each with how many todos it holds, and the field a new list is named in; a tap on a list shows it -->
<script lang="ts">
  import type { List } from '@todos/rpc'
  import Composer from '../molecules/Composer.svelte'
  import { SMART, listView, sameView, smart, type SmartId, type View } from '../lib/views'

  interface Props {
    lists: readonly List[]
    /** How many todos each smart list holds. */
    counts: Readonly<Record<SmartId, number>>
    /** How many todos are still to do in each list, by the list's id. */
    remaining: ReadonlyMap<number, number>
    /** The list that is shown; none is marked when left out (a narrow window showing only the lists). */
    selected?: View
    /** The name typed for a new list. */
    draft: string
    onselect: (view: View) => void
    ondraft: (name: string) => void
    onadd: () => void
  }

  let { lists, counts, remaining, selected, draft, onselect, ondraft, onadd }: Props = $props()

  const isSelected = (view: View): boolean => selected !== undefined && sameView(selected, view)
</script>

<nav class="sidebar" aria-label="Lists">
  <div class="lists">
    <!-- a row's own name leaves its value out: each is named with its count, as it reads -->
    <ui-list variant="sidebar" selection="single" label="Smart lists" data-testid="todos-smart">
      <ui-section>
        {#each SMART as s (s.id)}
          <ui-row
            heading={s.name}
            value={String(counts[s.id])}
            label="{s.name}, {counts[s.id]}"
            icon={s.icon}
            icon-color={s.color}
            selected={isSelected(smart(s.id))}
            onpress={() => onselect(smart(s.id))}
            data-testid="todos-smart-{s.id}"
          ></ui-row>
        {/each}
      </ui-section>
    </ui-list>
    <!-- an empty selectable list is not valid ARIA: the list is there once there is a list -->
    {#if lists.length > 0}
      <ui-list variant="sidebar" selection="single" label="My lists" data-testid="todos-lists">
        <ui-section header="My Lists">
          {#each lists as list (list.id)}
            <ui-row
              heading={list.name}
              value={String(remaining.get(list.id) ?? 0)}
              label="{list.name}, {remaining.get(list.id) ?? 0}"
              icon="checkmark"
              icon-color={list.color}
              selected={isSelected(listView(list.id))}
              onpress={() => onselect(listView(list.id))}
              data-testid="todos-list-{list.id}"
            ></ui-row>
          {/each}
        </ui-section>
      </ui-list>
    {/if}
  </div>
  <div class="new">
    <Composer {draft} label="New List" action="Add the list" testid="todos-new-list" {ondraft} {onadd} />
  </div>
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
  .lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
  .new {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--separator);
  }
</style>
