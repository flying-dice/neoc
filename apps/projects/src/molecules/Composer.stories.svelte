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
    canvas.getByTestId('projects-new-issue-input').shadowRoot!.querySelector('input')!

  /** Where something new is named. It keeps nothing: the text comes in, what was typed and the wish to add go out. */
  const { Story } = defineMeta({
    title: 'Molecules/Composer',
    component: Composer,
    tags: ['autodocs'],
    args: { draft: '', label: 'New Issue', action: 'Add the issue', testid: 'projects-new-issue', ondraft, onadd },
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
    await expect(canvas.getByTestId('projects-new-issue-add')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="With a title"
  args={{ draft: 'Fix the footer' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('Fix the footer')
    await expect(canvas.getByTestId('projects-new-issue-add')).not.toHaveAttribute('disabled')
  }}
/>

<!-- The same field names a new page. -->
<Story name="For a page" args={{ label: 'New Page', action: 'Add the page', testid: 'projects-new-page' }} />

<Story
  name="Typing and adding"
  tags={['play-only']}
  args={{ draft: 'Fix the footer' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), '!')
    await expect(ondraft).toHaveBeenCalledWith('Fix the footer!')
    await userEvent.type(input(canvas), '{Enter}')
    await expect(onadd).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('projects-new-issue-add'))
    await expect(onadd).toHaveBeenCalledTimes(2)
  }}
/>
