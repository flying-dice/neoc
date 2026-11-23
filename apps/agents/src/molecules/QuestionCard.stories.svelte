<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Item } from 'app:rpc'
  import QuestionCard from './QuestionCard.svelte'

  const onanswer = fn<(answer: string | null) => void>()
  const item = (status: string): Item => ({
    id: 1,
    chat: 'c',
    kind: 'interrupt',
    text: 'Send the report to everyone?',
    name: 'approval',
    data: JSON.stringify({ status }),
    done: status !== 'open'
  })

  /** A question an agent stopped at. It keeps what is being written; the answer or the refusal goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/QuestionCard',
    component: QuestionCard,
    tags: ['autodocs'],
    args: { item: item('open'), onanswer },
    beforeEach: () => {
      onanswer.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: { item: Item; onanswer: (answer: string | null) => void })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <QuestionCard {...args} />
  </div>
{/snippet}

<Story
  name="Open"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('agents-question-1-input').shadowRoot!.querySelector('input')!, 'Only to Ada{Enter}')
    await expect(onanswer).toHaveBeenCalledWith('Only to Ada')
    await userEvent.click(canvas.getByTestId('agents-question-1-decline'))
    await expect(onanswer).toHaveBeenCalledWith(null)
  }}
/>

<Story
  name="Answered"
  args={{ item: item('resolved') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-question-1')).toHaveTextContent('Answered')
    await expect(canvas.queryByTestId('agents-question-1-answer')).toBeNull()
  }}
/>

<Story name="Declined" args={{ item: item('cancelled') }} />
