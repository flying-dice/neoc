<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  type Item = VosApp.Agents.Item
  import ActivityCard from './ActivityCard.svelte'

  const item = (kind: Item['kind'], rest: Partial<Item> = {}): Item => ({ id: 1, chat: 'c', kind, text: '', name: '', data: '', done: true, ...rest })

  /** One thing an agent did that is not its words. What it thought and what a tool was given are closed until asked for. */
  const { Story } = defineMeta({
    title: 'Molecules/ActivityCard',
    component: ActivityCard,
    tags: ['autodocs'],
    args: { item: item('tool', { name: 'notes__add', text: '{"title":"Milk"}', data: JSON.stringify({ ours: true, result: '{"id":12}' }) }) },
    render: template
  })
</script>

{#snippet template(args: { item: Item })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ActivityCard {...args} />
  </div>
{/snippet}

<!-- A tool of an app on this machine, run here for the agent: opened, what it was given and what it answered. -->
<Story
  name="A tool of this machine"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('an app on this machine')
    await userEvent.click(canvas.getByTestId('agents-item-tool-1-summary'))
    await expect(canvas.getByTestId('agents-item-tool-1')).toHaveTextContent('"id": 12')
  }}
/>

<!-- The agent's own tool, still running where the agent is. -->
<Story name="A tool still running" args={{ item: item('tool', { name: 'web_search', text: '{"q":"milk"}', data: '{"ours":false}', done: false }) }} />

<Story name="A tool that failed" args={{ item: item('tool', { name: 'notes__add', text: '{}', data: JSON.stringify({ ours: true, error: 'no title', result: 'no title' }) }) }} />

<Story name="Reasoning" args={{ item: item('reasoning', { text: 'The person keeps a shopping list in Notes.\nI should add to it.' }) }} />

<Story name="A step in progress" args={{ item: item('step', { name: 'plan', done: false }) }} />

<Story
  name="A subagent that failed"
  args={{ item: item('subagent', { name: 'researcher', text: 'Looks for prices', data: JSON.stringify({ error: 'It ran out of time.' }) }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-item-subagent-1')).toHaveTextContent('It ran out of time.')
  }}
/>

<!-- An activity of a type this app has no drawing for: its type and its content, as the agent sent it. -->
<Story
  name="An activity"
  args={{ item: item('activity', { name: 'PLAN', data: JSON.stringify({ steps: ['find the list', 'add milk'] }) }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-item-activity-1')).toHaveTextContent('find the list')
  }}
/>

<Story name="An event of the agent's own" args={{ item: item('custom', { name: 'usage', data: '{"tokens":412}' }) }} />
