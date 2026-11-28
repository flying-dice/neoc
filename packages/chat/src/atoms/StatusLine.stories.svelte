<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import StatusLine from './StatusLine.svelte'

  /** The line over the composer while the agent's turn runs: a moving glyph in the accent, and what the agent is at. */
  const { Story } = defineMeta({
    title: 'Atoms/StatusLine',
    component: StatusLine,
    tags: ['autodocs'],
    args: { text: 'Thinking…' },
    render: template
  })
</script>

{#snippet template(args: { text: string | null })}
  <div style="width:390px;padding:8px 0;background:var(--bg);color:var(--label);font:var(--type-body)">
    <StatusLine {...args} testid="chat" />
  </div>
{/snippet}

<Story
  name="Thinking"
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('status')).toHaveTextContent('Thinking…')
  }}
/>

<!-- A tool is running: its title after the dot, cut short when it is too long for the line. -->
<Story name="A tool running" args={{ text: 'Thinking… · ls -la ~/Projects ~/Projects/.worktrees 2>&1 | head -50' }} />

<Story name="Tools running" args={{ text: 'Thinking… · 2 tools running' }} />

<!-- No turn runs: the region is there, and empty. -->
<Story
  name="Nothing running"
  args={{ text: null }}
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('status').textContent?.trim()).toBe('')
  }}
/>
