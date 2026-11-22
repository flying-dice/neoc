<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import StateDot from './StateDot.svelte'
  import type { Tone } from '../lib/format'

  interface Args {
    tone: Tone
    label: string
  }

  /** How a container or a stack is doing. The colour is never the only sign: not going is an empty ring, and the dot says its state in words. */
  const { Story } = defineMeta({
    title: 'Atoms/StateDot',
    component: StateDot,
    tags: ['autodocs'],
    args: { tone: 'on', label: 'Running' },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="padding:16px;background:var(--bg)">
    <StateDot {...args} />
  </div>
{/snippet}

<Story
  name="Going"
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('img', { name: 'Running' })).toBeVisible()
  }}
/>
<Story name="Not going" args={{ tone: 'off', label: 'Stopped' }} />
<Story name="In between" args={{ tone: 'busy', label: 'Restarting' }} />
<Story name="Broken" args={{ tone: 'bad', label: 'Dead' }} />
