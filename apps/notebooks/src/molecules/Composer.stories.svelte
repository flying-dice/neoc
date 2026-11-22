<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Composer from './Composer.svelte'

  interface Args {
    draft: string
    label: string
    action: string
    testid: string
    problem?: string | null
    ondraft: (text: string) => void
    onadd: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: { getByTestId(id: string): HTMLElement }): HTMLInputElement =>
    canvas.getByTestId('new-notebook-input').shadowRoot!.querySelector('input')!

  /** Where something new is named. It keeps nothing: the text comes in, what was typed and the wish to add go out. */
  const { Story } = defineMeta({
    title: 'Molecules/Composer',
    component: Composer,
    tags: ['autodocs'],
    args: { draft: '', label: 'New Notebook', action: 'Add the notebook', testid: 'new-notebook', ondraft, onadd },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:320px;background:var(--bg);color:var(--label)">
    <Composer {...args} />
  </div>
{/snippet}

<!-- Nothing typed: there is nothing to add. -->
<Story
  name="Empty"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('new-notebook-add')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="With a name"
  args={{ draft: 'Work' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('Work')
    await expect(canvas.getByTestId('new-notebook-add')).not.toHaveAttribute('disabled')
  }}
/>

<!-- A name that cannot be had: the field is marked, and the reason is under it. -->
<Story
  name="A name that is taken"
  args={{ draft: 'Notebook', problem: 'There is already a notebook called Notebook.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('new-notebook-problem')).toHaveTextContent('There is already a notebook called Notebook.')
    await expect(canvas.getByTestId('new-notebook-input')).toHaveAttribute('invalid')
  }}
/>

<Story
  name="Typing and adding"
  tags={['play-only']}
  args={{ draft: 'Work' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), '!')
    await expect(ondraft).toHaveBeenCalledWith('Work!')
    await userEvent.type(input(canvas), '{Enter}')
    await expect(onadd).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('new-notebook-add'))
    await expect(onadd).toHaveBeenCalledTimes(2)
  }}
/>
