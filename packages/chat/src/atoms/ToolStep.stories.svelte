<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Item } from '../types'
  import ToolStep from './ToolStep.svelte'
  import { DIFF, TOOL } from '../lib/samples'

  const item = (kind: Item['kind'], rest: Partial<Item> = {}): Item => ({ id: 1, chat: 'c', kind, text: '', name: '', data: '', done: true, ...rest })
  const onlink = fn<(url: string) => void>()

  /** One step of an agent's turn that is not its words: a quiet line, an icon for its kind and its title, opened in place. */
  const { Story } = defineMeta({
    title: 'Atoms/ToolStep',
    component: ToolStep,
    tags: ['autodocs'],
    args: { item: item('tool', TOOL), onlink },
    render: template
  })
</script>

{#snippet template(args: { item: Item; onlink: (url: string) => void })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ToolStep {...args} testid="chat" />
  </div>
{/snippet}

<!-- A tool call that changed a file: a line with the pencil and its title, no chips and no status word. -->
<Story
  name="A change to a file"
  play={async ({ canvas }) => {
    const step = canvas.getByTestId('chat-item-tool-1')
    await expect(step).toHaveTextContent('Add milk to list.md')
    await expect(step.querySelector('summary ui-icon')).toHaveAttribute('name', 'square.and.pencil')
    await expect(step).not.toHaveTextContent('edit')
    await expect(step).not.toHaveAttribute('open')
  }}
/>

<!-- Opened: where it is and the change, as a diff. -->
<Story
  name="A change to a file, opened"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('chat-item-tool-1-summary'))
    await expect(canvas.getByTestId('chat-item-1-diff-0')).toHaveTextContent(`Changed: ${DIFF.path}`)
    await expect(canvas.getByTestId('chat-item-1-diff-0')).toHaveTextContent('+Milk')
  }}
/>

<!-- A long title stays on its line, cut short with an ellipsis. -->
<Story
  name="A long title"
  args={{
    item: item('tool', {
      name: 'ls -la ~/Projects ~/Projects/.worktrees 2>&1 | head -50 && git -C ~/Projects/neoc log --oneline -20',
      data: JSON.stringify({ kind: 'execute', status: 'completed' })
    })
  }}
  play={async ({ canvas }) => {
    const summary = canvas.getByTestId('chat-item-tool-1-summary')
    await expect(summary.getBoundingClientRect().height).toBeLessThan(40)
  }}
/>

<!-- A tool call still running where the agent is: the line as any other (the line over the composer says it runs). -->
<Story
  name="A tool still running"
  args={{ item: item('tool', { name: 'Search for milk', data: JSON.stringify({ kind: 'search', status: 'in_progress' }), done: false }) }}
/>

<!-- A tool call that failed: a quiet "· failed" in red after its title; opened, what it was given and what it answered. -->
<Story
  name="A tool that failed"
  args={{
    item: item('tool', {
      name: 'Run the tests',
      data: JSON.stringify({ kind: 'execute', status: 'failed', input: '{"command":"cargo test"}', output: '{"exit":101}', content: [{ type: 'text', text: '1 test failed' }] })
    })
  }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('chat-item-tool-1')).toHaveTextContent('· failed')
    await userEvent.click(canvas.getByTestId('chat-item-tool-1-summary'))
    await expect(canvas.getByTestId('chat-item-tool-1')).toHaveTextContent('"command": "cargo test"')
    await expect(canvas.getByTestId('chat-item-tool-1')).toHaveTextContent('1 test failed')
  }}
/>

<!-- What the agent thought: a "Thought" line, opened to read it. -->
<Story
  name="A thought"
  args={{ item: item('reasoning', { text: 'The person keeps a shopping list in list.md.\nI should add to it.' }) }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('chat-item-reasoning-1-summary')).toHaveTextContent('Thought')
    await userEvent.click(canvas.getByTestId('chat-item-reasoning-1-summary'))
    await expect(canvas.getByTestId('chat-item-reasoning-1')).toHaveTextContent('I should add to it.')
  }}
/>
