<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Composer from './Composer.svelte'

  interface Args {
    draft: string
    busy: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onsend = fn<() => void>()
  const onstop = fn<() => void>()

  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: { getByTestId(id: string): HTMLElement }): HTMLInputElement =>
    canvas.getByTestId('agents-composer-input').shadowRoot!.querySelector('input')!

  /** Where a message is typed. It keeps nothing: the text comes in, what was typed and the wish to send go out. */
  const { Story } = defineMeta({
    title: 'Molecules/Composer',
    component: Composer,
    tags: ['autodocs'],
    args: { draft: '', busy: false, ondraft, onsend, onstop },
    beforeEach: () => {
      ondraft.mockClear()
      onsend.mockClear()
      onstop.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:420px;background:var(--bg);color:var(--label)">
    <Composer {...args} />
  </div>
{/snippet}

<!-- Nothing typed: there is nothing to send. -->
<Story
  name="Empty"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-composer-send')).toHaveAttribute('disabled')
  }}
/>

<Story
  name="With a message"
  args={{ draft: 'What is 12 times 7?' }}
  play={async ({ canvas }) => {
    await expect(input(canvas)).toHaveValue('What is 12 times 7?')
    await expect(canvas.getByTestId('agents-composer-send')).not.toHaveAttribute('disabled')
  }}
/>

<!-- The agent is answering: the button stops it. -->
<Story
  name="While the agent answers"
  args={{ busy: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('agents-composer-stop'))
    await expect(onstop).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Typing reports the text; Enter and the button both send. -->
<Story
  name="Typing and sending"
  tags={['play-only']}
  args={{ draft: 'hi' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(input(canvas), '!')
    await expect(ondraft).toHaveBeenLastCalledWith('hi!')
    await userEvent.keyboard('{Enter}')
    await expect(onsend).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('agents-composer-send'))
    await expect(onsend).toHaveBeenCalledTimes(2)
  }}
/>
