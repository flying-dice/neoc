<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Item = VosApp.Agents.Item
  import QuestionCard from './QuestionCard.svelte'
  import { OPTIONS } from '../lib/fakeApi'
  import type { PermissionOption } from '../lib/items'

  const onanswer = fn<(option: PermissionOption) => void>()
  const item = (done: boolean, chosen?: string): Item => ({
    id: 1,
    chat: 'c',
    kind: 'permission',
    text: 'Add milk to list.md',
    name: '',
    data: JSON.stringify({ kind: 'edit', options: OPTIONS, ...(chosen ? { chosen } : {}) }),
    done
  })

  /** A permission an agent asks for during its turn. It keeps nothing: the option picked goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/QuestionCard',
    component: QuestionCard,
    tags: ['autodocs'],
    args: { item: item(false), onanswer },
    beforeEach: () => {
      onanswer.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: { item: Item; onanswer: (option: PermissionOption) => void })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <QuestionCard {...args} />
  </div>
{/snippet}

<!-- Open: the agent's own options, the ones that allow filled, the ones that refuse in red. -->
<Story
  name="Open"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-question-1')).toHaveTextContent('Add milk to list.md')
    await userEvent.click(canvas.getByTestId('agents-question-1-option-0'))
    await expect(onanswer).toHaveBeenCalledWith(OPTIONS[0])
    await userEvent.click(canvas.getByTestId('agents-question-1-option-2'))
    await expect(onanswer).toHaveBeenCalledWith(OPTIONS[2])
  }}
/>

<Story
  name="Answered"
  args={{ item: item(true, 'always') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-question-1-answer')).toHaveTextContent('Answered: Always allow')
    await expect(canvas.queryByTestId('agents-question-1-option-0')).toBeNull()
  }}
/>

<!-- The turn ended (stopped, or the backend restarted) before the person answered. -->
<Story name="Not answered" args={{ item: item(true) }} />
