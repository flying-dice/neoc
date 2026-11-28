<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import type { Item } from '../types'
  import PlanSteps from './PlanSteps.svelte'
  import { PLAN } from '../lib/samples'

  const item = (rest: Partial<Item> = {}): Item => ({ id: 1, chat: 'c', text: '', name: '', data: '', done: true, ...PLAN, ...rest })

  /** The agent's plan for its turn: a quiet checklist, each step ticked when done. */
  const { Story } = defineMeta({
    title: 'Molecules/PlanSteps',
    component: PlanSteps,
    tags: ['autodocs'],
    args: { item: item() },
    render: template
  })
</script>

{#snippet template(args: { item: Item })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <PlanSteps {...args} testid="chat" />
  </div>
{/snippet}

<!-- One step done, one being done: a screen reader hears where each is. -->
<Story
  name="A plan"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('chat-item-plan-1')).toHaveTextContent('Done Find the list')
    await expect(canvas.getByTestId('chat-item-plan-1')).toHaveTextContent('Doing Add milk')
  }}
/>

<!-- Steps still to do, and one too long for the line, which wraps. -->
<Story
  name="Still to do"
  args={{
    item: item({
      data: JSON.stringify({
        entries: [
          { content: 'Read the failing test and the code it covers, then find where the two disagree about the order of items', priority: 'high', status: 'in_progress' },
          { content: 'Fix it', priority: 'medium', status: 'pending' },
          { content: 'Run the tests', priority: 'low', status: 'pending' }
        ]
      })
    })
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('chat-item-plan-1')).toHaveTextContent('To do Fix it')
  }}
/>
