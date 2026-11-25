<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Folder } from '@notes/rpc'
  import FolderSidebar from './FolderSidebar.svelte'
  import { ALL, folderView, type View } from '../lib/views'

  interface Args {
    folders: readonly Folder[]
    counts: { all: number; deleted: number; folders: ReadonlyMap<number, number> }
    selected?: View
    draft: string
    problem?: string | null
    onselect: (view: View) => void
    ondraft: (name: string) => void
    onadd: () => void
  }

  const onselect = fn<(view: View) => void>()
  const FOLDERS: Folder[] = [
    { id: 1, name: 'Notes' },
    { id: 2, name: 'Work' },
    { id: 3, name: 'Recipes' }
  ]

  /** The folders, each with how many notes it holds. It keeps nothing: what is shown comes in, the tap goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/FolderSidebar',
    component: FolderSidebar,
    tags: ['autodocs'],
    args: {
      folders: FOLDERS,
      counts: { all: 12, deleted: 2, folders: new Map([[1, 7], [2, 5]]) },
      selected: ALL,
      draft: '',
      onselect,
      ondraft: fn(),
      onadd: fn()
    },
    beforeEach: () => {
      onselect.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:260px;height:480px;color:var(--label);font:var(--type-body)">
    <FolderSidebar {...args} />
  </div>
{/snippet}

<Story
  name="All Notes shown"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-all')).toHaveProperty('value', '12')
    await expect(canvas.getByTestId('notes-all')).toHaveAttribute('selected')
    await expect(canvas.getByTestId('notes-folder-3')).toHaveProperty('value', '0')
    await expect(canvas.getByTestId('notes-deleted')).toHaveProperty('value', '2')
  }}
/>

<Story name="A folder shown" args={{ selected: folderView(2) }} />

<!-- A narrow window shows only the folders: none is marked. -->
<Story name="Nothing marked" args={{ selected: undefined }} />

<Story name="A name that is taken" args={{ draft: 'work', problem: 'There is already a folder called work.' }} />

<Story
  name="Picking a folder"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notes-folder-2'))
    await expect(onselect).toHaveBeenCalledWith(folderView(2))
  }}
/>
