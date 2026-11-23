<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Engine } from '../types'
  import EngineDown from './EngineDown.svelte'

  interface Args {
    engine: Engine
    draft: string
    ondraft: (address: string) => void
    onconnect: () => void
    testid: string
  }

  const ondraft = fn<(address: string) => void>()
  const onconnect = fn<() => void>()

  /** Docker cannot be reached: why, and what to do about it. */
  const { Story } = defineMeta({
    title: 'Organisms/EngineDown',
    component: EngineDown,
    tags: ['autodocs'],
    args: {
      engine: { reachable: false, version: '', host: '', problem: 'Docker is not running on this machine, or cannot be reached.' },
      draft: '',
      ondraft,
      onconnect,
      testid: 'dns'
    },
    beforeEach: () => {
      ondraft.mockClear()
      onconnect.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:460px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <EngineDown {...args} />
  </div>
{/snippet}

<!-- This machine's own Docker is not running: starting it and trying again is the way on. -->
<Story
  name="Not running here"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-down')).toHaveProperty('message', 'Docker is not running on this machine, or cannot be reached.')
    await userEvent.click(canvas.getByTestId('dns-host-connect'))
    await expect(onconnect).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="Not reachable elsewhere"
  args={{
    engine: { reachable: false, version: '', host: 'tcp://nas.local:2375', problem: 'Docker cannot be reached at tcp://nas.local:2375.' },
    draft: 'tcp://nas.local:2375'
  }}
/>
