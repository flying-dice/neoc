<!-- @organism FolderSidebar — All Notes, the person's folders and Recently Deleted, each with how many notes it holds, and the field a new folder is named in; a tap on one shows it -->
<script lang="ts">
  import type { Folder } from 'app:rpc'
  import Composer from '../molecules/Composer.svelte'
  import { ALL, DELETED, folderView, sameView, type View } from '../lib/views'

  interface Props {
    folders: readonly Folder[]
    /** How many notes there are: in all, deleted, and in each folder by the folder's id. */
    counts: { readonly all: number; readonly deleted: number; readonly folders: ReadonlyMap<number, number> }
    /** What is shown; nothing is marked when left out (a narrow window showing only the folders). */
    selected?: View
    /** The name typed for a new folder, and what is wrong with it. */
    draft: string
    problem?: string | null
    onselect: (view: View) => void
    ondraft: (name: string) => void
    onadd: () => void
  }

  let { folders, counts, selected, draft, problem = null, onselect, ondraft, onadd }: Props = $props()

  const isSelected = (view: View): boolean => selected !== undefined && sameView(selected, view)
  const held = (folder: Folder): number => counts.folders.get(folder.id) ?? 0
</script>

<nav class="sidebar" aria-label="Folders">
  <div class="folders">
    <!-- a row's own name leaves its value out: each is named with its count, as it reads -->
    <ui-list variant="sidebar" selection="single" label="Folders" data-testid="notes-folders">
      <ui-section>
        <ui-row
          heading="All Notes"
          value={String(counts.all)}
          label="All Notes, {counts.all}"
          icon="rectangle.stack"
          icon-color="yellow"
          selected={isSelected(ALL)}
          onpress={() => onselect(ALL)}
          data-testid="notes-all"
        ></ui-row>
      </ui-section>
      <ui-section header="Folders">
        {#each folders as folder (folder.id)}
          <ui-row
            heading={folder.name}
            value={String(held(folder))}
            label="{folder.name}, {held(folder)}"
            icon="folder"
            icon-color="yellow"
            selected={isSelected(folderView(folder.id))}
            onpress={() => onselect(folderView(folder.id))}
            data-testid="notes-folder-{folder.id}"
          ></ui-row>
        {/each}
      </ui-section>
      <ui-section>
        <ui-row
          heading="Recently Deleted"
          value={String(counts.deleted)}
          label="Recently Deleted, {counts.deleted}"
          icon="trash"
          icon-color="gray"
          selected={isSelected(DELETED)}
          onpress={() => onselect(DELETED)}
          data-testid="notes-deleted"
        ></ui-row>
      </ui-section>
    </ui-list>
  </div>
  <div class="new">
    <Composer {draft} {problem} label="New Folder" action="Add the folder" testid="notes-new-folder" {ondraft} {onadd} />
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
  .folders {
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
