<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import StateDot from './StateDot.svelte'
  import type { Tone } from '../types'

  interface Args {
    tone: Tone
    label: string
  }

  /** How something in Docker is doing. The colour is never the only sign: not going is an empty ring, and the dot says its state in words. */
  const { Story } = defineMeta({
    title: 'Atoms/StateDot',
    component: StateDot,
    tags: ['autodocs'],
    args: { tone: 'on', label: 'Answering on port 53' },
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
    await expect(canvas.getByRole('img', { name: 'Answering on port 53' })).toBeVisible()
  }}
/>
<Story name="Not going" args={{ tone: 'off', label: 'Off' }} />
<Story name="In between" args={{ tone: 'busy', label: 'Running with older settings' }} />
<Story name="Broken" args={{ tone: 'bad', label: 'Not running' }} />
