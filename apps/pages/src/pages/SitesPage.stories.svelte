<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import SitesPage from './SitesPage.svelte'
  import { PORT_TAKEN, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'
  import { Pages } from '../lib/pages.svelte'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host or a Docker. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Pages app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Pages',
    component: SitesPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = { click(element: Element): Promise<void>; type(element: Element, text: string): Promise<void> }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const says = (canvas: Canvas, id: string, text: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))
  /** Bring a tab to the front. */
  async function show(canvas: Canvas, user: User, tab: string): Promise<void> {
    await appears(canvas, 'pages-tab')
    await user.click(inner(canvas, 'pages-tab', `[data-testid="pages-tab-${tab}"]`))
  }
  /** Type a new site into the form. */
  async function type(canvas: Canvas, user: User, typed: { name: string; folder: string; port?: string; hostnames?: string }): Promise<void> {
    await appears(canvas, 'pages-add')
    await user.click(canvas.getByTestId('pages-add'))
    await appears(canvas, 'pages-site-name')
    await user.type(inner(canvas, 'pages-site-name', 'input'), typed.name)
    await user.type(inner(canvas, 'pages-site-folder', 'input'), typed.folder)
    if (typed.port) await user.type(inner(canvas, 'pages-site-port', 'input'), typed.port)
    if (typed.hostnames) await user.type(inner(canvas, 'pages-site-hostnames', 'input'), typed.hostnames)
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const pages = new Pages(backend.rpc, backend.host)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <SitesPage {pages} here="nas.local" />
  </AppWindow>
{/snippet}

<!-- A home server: two sites are served, one is kept and off. -->
<Story
  name="Serving"
  play={async ({ canvas }) => {
    await says(canvas, 'pages-state', 'Serving 2 sites')
    await says(canvas, 'pages-sites-summary', '3 sites')
    await says(canvas, 'pages-site-1-open-0', 'nas.local:8081')
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<!-- A site is typed and added: it is in the list, served, and the form is gone. -->
<Story
  name="Adding a site"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await type(canvas, userEvent, { name: 'Photos', folder: '/Users/me/Sites/photos', port: '8082', hostnames: 'photos.home' })
    await userEvent.click(canvas.getByTestId('pages-site-save'))
    await says(canvas, 'pages-sites-summary', '4 sites')
    await says(canvas, 'pages-site-4-name', 'Photos')
    await says(canvas, 'pages-state', 'Serving 3 sites')
  }}
/>

<Story
  name="The form for a new site"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'pages-add')
    await userEvent.click(canvas.getByTestId('pages-add'))
    await appears(canvas, 'pages-site-folder')
  }}
/>

<!-- A port another site has is refused in the backend's words, and what was typed stays. -->
<Story
  name="A port that is taken by another site"
  play={async ({ canvas, userEvent }) => {
    await type(canvas, userEvent, { name: 'Photos', folder: '/Users/me/Sites/photos', port: '8081' })
    await userEvent.click(canvas.getByTestId('pages-site-save'))
    await says(canvas, 'pages-failure', 'Port 8081 is the port of Blog.')
    await appears(canvas, 'pages-site-name')
  }}
/>

<Story
  name="Removing a site"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'pages-site-2-remove')
    await userEvent.click(canvas.getByTestId('pages-site-2-remove'))
    await gone(canvas, 'pages-site-2')
    await says(canvas, 'pages-state', 'Serving 1 site')
  }}
/>

<Story
  name="No sites yet"
  args={{ backend: () => createFakeBackend({ sites: [] }) }}
  play={async ({ canvas }) => {
    await says(canvas, 'pages-state', 'No sites yet')
    await appears(canvas, 'pages-empty')
  }}
/>

<!-- Docker would not start the container: there are sites to serve, the server is not running, and says why. -->
<Story
  name="A port is taken on the machine"
  args={{ backend: () => createFakeBackend({ refusing: PORT_TAKEN }) }}
  play={async ({ canvas, userEvent }) => {
    await says(canvas, 'pages-state', 'Not running')
    await userEvent.click(canvas.getByTestId('pages-apply'))
    await says(canvas, 'pages-attention', 'port is already allocated')
  }}
/>

<Story
  name="A folder is not there"
  args={{ backend: () => createFakeBackend({ missing: ['/Users/me/Sites/blog'] }) }}
  play={async ({ canvas }) => {
    await says(canvas, 'pages-site-1-problem', 'The folder is not there.')
    await says(canvas, 'pages-state', 'Serving 1 site')
  }}
/>

<Story
  name="Settings"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'settings')
    await appears(canvas, 'pages-port')
    await says(canvas, 'pages-engine', 'Docker 27.3.1')
  }}
/>

<Story
  name="Log"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'log')
    await says(canvas, 'pages-log', 'start worker processes')
  }}
/>

<Story
  name="Docker is not running"
  args={{ backend: () => createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'pages-down')
  }}
/>

<Story
  name="The host is not answering"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'pages-failure', 'The host is not answering.')
  }}
/>
