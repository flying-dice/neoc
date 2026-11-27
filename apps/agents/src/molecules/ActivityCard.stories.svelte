<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  type Item = VosApp.Agents.Item
  import ActivityCard from './ActivityCard.svelte'
  import { DIFF, PLAN, TOOL } from '../lib/fakeApi'

  const item = (kind: Item['kind'], rest: Partial<Item> = {}): Item => ({ id: 1, chat: 'c', kind, text: '', name: '', data: '', done: true, ...rest })

  /** One thing an agent did that is not its words. What it thought and what a tool did are closed until asked for. */
  const { Story } = defineMeta({
    title: 'Molecules/ActivityCard',
    component: ActivityCard,
    tags: ['autodocs'],
    args: { item: item('tool', TOOL) },
    render: template
  })
</script>

{#snippet template(args: { item: Item })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ActivityCard {...args} />
  </div>
{/snippet}

<!-- A tool call that changed a file: opened, where it is and the change, as a diff. -->
<Story
  name="A change to a file"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('Add milk to list.md')
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('edit')
    await userEvent.click(canvas.getByTestId('agents-item-tool-1-summary'))
    await expect(canvas.getByTestId('agents-item-1-diff-0')).toHaveTextContent(`Changed: ${DIFF.path}`)
    await expect(canvas.getByTestId('agents-item-1-diff-0')).toHaveTextContent('+Milk')
  }}
/>

<!-- A tool call still running where the agent is. -->
<Story
  name="A tool still running"
  args={{ item: item('tool', { name: 'Search for milk', data: JSON.stringify({ kind: 'search', status: 'in_progress' }), done: false }) }}
/>

<!-- A tool call that failed, opened: what it was given and what it answered. -->
<Story
  name="A tool that failed"
  args={{
    item: item('tool', {
      name: 'Run the tests',
      data: JSON.stringify({ kind: 'execute', status: 'failed', input: '{"command":"cargo test"}', output: '{"exit":101}', content: [{ type: 'text', text: '1 test failed' }] })
    })
  }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('failed')
    await userEvent.click(canvas.getByTestId('agents-item-tool-1-summary'))
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('"command": "cargo test"')
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('1 test failed')
  }}
/>

<Story name="Reasoning" args={{ item: item('reasoning', { text: 'The person keeps a shopping list in list.md.\nI should add to it.' }) }} />

<!-- The agent's plan for its turn: open, each step with where it is. -->
<Story
  name="A plan"
  args={{ item: item('plan', PLAN) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-item-plan-1')).toHaveTextContent('Done Find the list')
    await expect(canvas.getByTestId('agents-item-plan-1')).toHaveTextContent('Doing Add milk')
  }}
/>
