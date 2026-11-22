<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Composer from './Composer.svelte'

  interface Args {
    draft: string
    label: string
    action: string
    testid: string
    ondraft: (text: string) => void
    onadd: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: { getByTestId(id: string): HTMLElement }): HTMLInputElement =>
    canvas.getByTestId('todos-new-input').shadowRoot!.querySelector('input')!

  /** Where something new is named. It keeps nothing: the text comes in, what was typed and the wish to add go out. */
  const { Story } = defineMeta({
    title: 'Molecules/Composer',
    component: Composer,
    tags: ['autodocs'],
    args: { draft: '', label: 'New Todo', action: 'Add the todo', testid: 'todos-new', ondraft, onadd },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:420px;background:var(--bg);color:var(--label)">
    <Composer {...args} />
  </div>
{/snippet}

<!-- Nothing typed: there is nothing to add. -->
<Story
  name="Empty"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('todos-new-add')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="With a title"
  args={{ draft: 'Buy milk' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('Buy milk')
    await expect(canvas.getByTestId('todos-new-add')).not.toHaveAttribute('disabled')
  }}
/>

<!-- The same field names a new list. -->
<Story name="For a list" args={{ label: 'New List', action: 'Add the list', testid: 'todos-new-list' }} />

<Story
  name="Typing and adding"
  tags={['play-only']}
  args={{ draft: 'Buy milk' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), '!')
    await expect(ondraft).toHaveBeenCalledWith('Buy milk!')
    await userEvent.type(input(canvas), '{Enter}')
    await expect(onadd).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('todos-new-add'))
    await expect(onadd).toHaveBeenCalledTimes(2)
  }}
/>
