<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import DnsPage from './DnsPage.svelte'
  import { Dns } from '../lib/dns.svelte'
  import { PORT_TAKEN, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host or a Docker. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The DNS app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/DNS',
    component: DnsPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = { click(element: Element): Promise<void>; type(element: Element, text: string): Promise<void>; tab(): Promise<void> }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const says = (canvas: Canvas, id: string, text: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))
  /** Bring a tab to the front. */
  async function show(canvas: Canvas, user: User, tab: string): Promise<void> {
    await appears(canvas, 'dns-tab')
    await user.click(inner(canvas, 'dns-tab', `[data-testid="dns-tab-${tab}"]`))
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const dns = new Dns(backend.rpc, backend.events)}
  <AppWindow title={app.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <DnsPage {dns} />
  </AppWindow>
{/snippet}

<!-- A home network: the server answers, and these are its names. -->
<Story
  name="Answering"
  play={async ({ canvas }) => {
    await says(canvas, 'dns-state', 'Answering on port 53')
    await says(canvas, 'dns-records-summary', '4 names')
    await appears(canvas, 'dns-record-1')
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<!-- A name is typed and added: it is in the list, and the fields are empty again. -->
<Story
  name="Adding a name"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'dns-record-name')
    await userEvent.type(inner(canvas, 'dns-record-name', 'input'), 'tv.home')
    await userEvent.type(inner(canvas, 'dns-record-address', 'input'), '192.168.1.40')
    await userEvent.click(canvas.getByTestId('dns-record-add'))
    await says(canvas, 'dns-records-summary', '5 names')
    await says(canvas, 'dns-record-5-name', 'tv.home')
  }}
/>

<Story
  name="Removing a name"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'dns-record-3-remove')
    await userEvent.click(canvas.getByTestId('dns-record-3-remove'))
    await gone(canvas, 'dns-record-3')
    await says(canvas, 'dns-records-summary', '3 names')
  }}
/>

<!-- An address that is none is said in the footer of the form once its field is left, and Add cannot be pressed: the window has no banner for it. -->
<Story
  name="An address that is none"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'dns-record-name')
    await userEvent.type(inner(canvas, 'dns-record-name', 'input'), 'tv.home')
    await userEvent.type(inner(canvas, 'dns-record-address', 'input'), 'upstairs')
    await userEvent.tab()
    await waitFor(() => expect(canvas.getByTestId('dns-record-form')).toHaveProperty('footer', 'Not an IP address, like 192.168.1.10 or fd00::10.'))
    await expect(canvas.getByTestId('dns-record-add')).toHaveProperty('disabled', true)
    await expect(canvas.queryByTestId('dns-failure')).toBeNull()
    await says(canvas, 'dns-records-summary', '4 names')
  }}
/>

<Story
  name="Off, with no names yet"
  args={{ backend: () => createFakeBackend({ enabled: false, records: [] }) }}
  play={async ({ canvas }) => {
    await says(canvas, 'dns-state', 'Off')
    await appears(canvas, 'dns-empty')
  }}
/>

<!-- Docker would not start the container: the server is wanted on, is not running, and says why. -->
<Story
  name="The port is taken"
  args={{ backend: () => createFakeBackend({ refusing: PORT_TAKEN }) }}
  play={async ({ canvas, userEvent }) => {
    await says(canvas, 'dns-state', 'Not running')
    await userEvent.click(canvas.getByTestId('dns-apply'))
    await says(canvas, 'dns-attention', 'port is already allocated')
  }}
/>

<Story
  name="Settings"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'settings')
    await appears(canvas, 'dns-port')
    await says(canvas, 'dns-engine', 'Docker 27.3.1')
  }}
/>

<Story
  name="Log"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'log')
    await says(canvas, 'dns-log', 'dnsmasq: started')
  }}
/>

<Story
  name="Docker is not running"
  args={{ backend: () => createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'dns-down')
  }}
/>

<Story
  name="The host is not answering"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'dns-failure', 'The host is not answering.')
  }}
/>
