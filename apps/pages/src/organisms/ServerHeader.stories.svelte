<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Server = VosApp.Pages.Server
  type Site = VosApp.Pages.Site
  import ServerHeader from './ServerHeader.svelte'
  import { IMAGE, PORT_TAKEN, SITES } from '../lib/fakeApi'

  interface Args {
    server: Server
    sites: readonly Site[]
    working: boolean
    onapply: () => void
  }

  const onapply = fn<() => void>()
  const server = (change: Partial<Server> = {}): Server => ({
    wanted: true,
    state: 'running',
    status: 'Up 2 hours',
    current: true,
    image: IMAGE,
    problem: '',
    ...change
  })

  /** The web server at the top of the window: how it is doing. */
  const { Story } = defineMeta({
    title: 'Organisms/ServerHeader',
    component: ServerHeader,
    tags: ['autodocs'],
    args: { server: server(), sites: SITES, working: false, onapply },
    beforeEach: () => {
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
  name="Serving"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-state')).toHaveTextContent('Serving 2 sites')
    await expect(canvas.queryByTestId('pages-attention')).toBeNull()
  }}
/>

<Story
  name="No sites yet"
  args={{ server: server({ wanted: false, state: 'absent', status: '', current: false }), sites: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-state')).toHaveTextContent('No sites yet')
  }}
/>

<Story name="Being made" args={{ working: true }} />

<!-- Docker refused to start the container: its own words, and the way to try again once the port is free. -->
<Story
  name="A port is taken"
  args={{ server: server({ state: 'stopped', status: 'Created', problem: PORT_TAKEN }) }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('pages-state')).toHaveTextContent('Not running')
    await expect(canvas.getByTestId('pages-attention')).toHaveTextContent('port is already allocated')
    await userEvent.click(canvas.getByTestId('pages-apply'))
    await expect(onapply).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="Stopped by itself" args={{ server: server({ state: 'stopped', status: 'Exited (1) 5 seconds ago' }) }} />
<Story name="Running with older settings" args={{ server: server({ current: false }) }} />
