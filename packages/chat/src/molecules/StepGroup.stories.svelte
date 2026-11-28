<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Item } from '../types'
  import StepGroup from './StepGroup.svelte'
  import { DIFF } from '../lib/samples'

  const onlink = fn<(url: string) => void>()
  const step = (id: number, name: string, kind: string, rest: Record<string, unknown> = {}, done = true): Item => ({
    id,
    chat: 'c',
    kind: 'tool',
    text: '',
    name,
    data: JSON.stringify({ kind, status: done ? 'completed' : 'in_progress', ...rest }),
    done
  })
  const WORK: Item[] = [
    step(1, 'Create notes.md', 'edit', { content: [{ ...DIFF, path: '/home/ada/project/notes.md', new_file: true, patch: '+# Notes\n' }] }),
    step(2, 'Add milk to list.md', 'edit', { content: [DIFF] }),
    step(3, 'Fix the title of todo.md', 'edit'),
    step(4, 'Remove old.md', 'delete'),
    step(5, 'Remove older.md', 'delete', { status: 'failed', output: '{"error":"permission denied"}' })
  ]

  /** A run of the agent's steps between its words: one quiet row counting what they did, opened in place. */
  const { Story } = defineMeta({
    title: 'Molecules/StepGroup',
    component: StepGroup,
    tags: ['autodocs'],
    args: { steps: WORK, onlink },
    render: template
  })
</script>

{#snippet template(args: { steps: Item[]; onlink: (url: string) => void })}
  <div style="display:flex;flex-direction:column;width:390px;padding:16px;box-sizing:border-box;background:var(--bg);color:var(--label);font:var(--type-body)">
    <StepGroup {...args} testid="chat" />
  </div>
{/snippet}

<!-- Closed: one row, what the steps did counted by kind, and a quiet count of the ones that failed. -->
<Story
  name="Files changed"
  play={async ({ canvas }) => {
    const summary = canvas.getByTestId('chat-steps-1-summary')
    await expect(summary).toHaveTextContent('Created 1 file · Edited 2 files · Deleted 2 files')
    await expect(summary).toHaveTextContent('· 1 failed')
    await expect(summary.getBoundingClientRect().height).toBeLessThan(40)
    await expect(canvas.queryByTestId('chat-item-tool-1')).toBeNull()
  }}
/>

<!-- Opened: a line for each step; a step opens in turn to what it changed. -->
<Story
  name="Opened"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('chat-steps-1-summary'))
    await expect(canvas.getByTestId('chat-item-tool-2-summary')).toBeVisible()
    await expect(canvas.getByTestId('chat-item-tool-5-summary')).toHaveTextContent('· failed')
    await userEvent.click(canvas.getByTestId('chat-item-tool-2-summary'))
    await expect(canvas.getByTestId('chat-item-2-diff-0')).toHaveTextContent('+Milk')
  }}
/>

<!-- Still going: the run counts the step that is running with the rest. -->
<Story
  name="Commands and reads, one still running"
  args={{
    steps: [
      step(1, 'cargo check -p neoc-vm-std', 'execute'),
      { id: 2, chat: 'c', kind: 'reasoning', text: 'The error is in the parser.', name: '', data: '', done: true },
      step(3, 'Read parser.rs', 'read'),
      step(4, 'cargo test -p neoc-vm-std parser', 'execute', {}, false)
    ]
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('chat-steps-1-summary')).toHaveTextContent('Ran 2 commands · Thought once · Read 1 file')
    await expect(canvas.getByTestId('chat-steps-1-summary').querySelector('ui-icon')).toHaveAttribute('name', 'terminal')
  }}
/>
