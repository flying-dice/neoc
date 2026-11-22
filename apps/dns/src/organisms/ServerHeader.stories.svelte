<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Server, Settings } from 'app:rpc'
  import ServerHeader from './ServerHeader.svelte'
  import { IMAGE, PORT_TAKEN, SETTINGS } from '../lib/fakeApi'

  interface Args {
    server: Server
    settings: Settings
    working: boolean
    onenable: (enabled: boolean) => void
    onapply: () => void
  }

  const onenable = fn<(enabled: boolean) => void>()
  const onapply = fn<() => void>()
  const server = (change: Partial<Server> = {}): Server => ({
    enabled: true,
    state: 'running',
    status: 'Up 2 hours',
    current: true,
    image: IMAGE,
    problem: '',
    ...change
  })

  /** The server at the top of the window: how it is doing, and its switch. */
  const { Story } = defineMeta({
    title: 'Organisms/ServerHeader',
    component: ServerHeader,
    tags: ['autodocs'],
    args: { server: server(), settings: SETTINGS, working: false, onenable, onapply },
    beforeEach: () => {
      onenable.mockClear()
      onapply.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ServerHeader {...args} />
  </div>
{/snippet}

<Story
  name="Answering"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-state')).toHaveTextContent('Answering on port 53')
    await expect(canvas.queryByTestId('dns-attention')).toBeNull()
  }}
/>

<Story
  name="Off"
  args={{ server: server({ enabled: false, state: 'absent', status: '', current: false }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-state')).toHaveTextContent('Off')
  }}
/>

<Story name="Being made" args={{ working: true }} />

<!-- Docker refused to start the container: its own words, and the way to try again once the port is free. -->
<Story
  name="The port is taken"
  args={{ server: server({ state: 'stopped', status: 'Created', problem: PORT_TAKEN }) }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-state')).toHaveTextContent('Not running')
    await expect(canvas.getByTestId('dns-attention')).toHaveTextContent('port is already allocated')
    await userEvent.click(canvas.getByTestId('dns-apply'))
    await expect(onapply).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Stopped by itself" args={{ server: server({ state: 'stopped', status: 'Exited (2) 5 seconds ago' }) }} />
<Story name="Running with older settings" args={{ server: server({ current: false }) }} />
