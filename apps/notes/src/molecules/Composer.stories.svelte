<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Composer from './Composer.svelte'

  interface Args {
    draft: string
    label: string
    action: string
    problem?: string | null
    testid: string
    ondraft: (text: string) => void
    onadd: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: { getByTestId(id: string): HTMLElement }): HTMLInputElement =>
    canvas.getByTestId('notes-new-folder-input').shadowRoot!.querySelector('input')!

  /** Where a new folder is named. It keeps nothing: the text comes in, what was typed and the wish to add go out. */
  const { Story } = defineMeta({
    title: 'Molecules/Composer',
    component: Composer,
    tags: ['autodocs'],
    args: { draft: '', label: 'New Folder', action: 'Add the folder', testid: 'notes-new-folder', ondraft, onadd },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:260px;padding:12px;background:var(--bg-secondary);color:var(--label)">
    <Composer {...args} />
  </div>
{/snippet}

<!-- Nothing typed: there is nothing to add. -->
<Story
  name="Empty"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-new-folder-add')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="With a name"
  args={{ draft: 'Recipes' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('Recipes')
    await expect(canvas.getByTestId('notes-new-folder-add')).not.toHaveAttribute('disabled')
  }}
/>

<!-- The name is taken: said under the field, the field marked, and nothing to add until it is right. -->
<Story
  name="A name that is taken"
  args={{ draft: 'Work', problem: 'There is already a folder called Work.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notes-new-folder-problem')).toHaveTextContent('There is already a folder called Work.')
    await expect(canvas.getByTestId('notes-new-folder-input')).toHaveAttribute('invalid')
    await expect(canvas.getByTestId('notes-new-folder-add')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="Typing and adding"
  tags={['play-only']}
  args={{ draft: 'Recipes' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), '!')
    await expect(ondraft).toHaveBeenCalledWith('Recipes!')
    await userEvent.type(input(canvas), '{Enter}')
    await expect(onadd).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notes-new-folder-add'))
    await expect(onadd).toHaveBeenCalledTimes(2)
  }}
/>
