<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import ContainersPage from './ContainersPage.svelte'
  import { Containers } from '../lib/containers.svelte'
  import { container, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake Docker in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Containers app in its window, on a fake Docker: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Containers',
    component: ContainersPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = {
    click(element: Element): Promise<void>
    type(element: Element, text: string, options?: { initialSelectionStart: number; initialSelectionEnd: number }): Promise<void>
  }
  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const says = (canvas: Canvas, id: string, text: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))

  /** A narrow window shows the stacks first: pick one there. In a wide one the same tap only changes what is shown. */
  async function show(canvas: Canvas, user: User, id: string): Promise<void> {
    await appears(canvas, id)
    await user.click(canvas.getByTestId(id))
  }
  /** Look at a container of what is shown. */
  async function inspect(canvas: Canvas, user: User, name: string): Promise<void> {
    await appears(canvas, `containers-item-${name}-open`)
    await user.click(canvas.getByTestId(`containers-item-${name}-open`))
    await appears(canvas, 'containers-inspector-name')
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const containers = new Containers(backend.rpc, backend.events)}
  <AppWindow
    title={definition.info.title}
    width={args.width}
    height={args.height}
    minWidth={app.window.minW}
    minHeight={app.window.minH}
  >
    <ContainersPage {containers} />
  </AppWindow>
{/snippet}

<!-- A machine with two Compose stacks and two containers on their own: every container, and the stacks beside them. -->
<Story
  name="Every container"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await says(canvas, 'containers-title', 'All Containers')
    await says(canvas, 'containers-summary', '4 of 6 containers running')
    await appears(canvas, 'containers-item-pihole')
  }}
/>

<!-- One stack: its own containers, and the buttons that act on all of it. -->
<Story
  name="A stack"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-stack-media')
    await says(canvas, 'containers-title', 'media')
    await says(canvas, 'containers-summary', '1 of 2 containers running')
    await appears(canvas, 'containers-stack-actions')
    await gone(canvas, 'containers-item-pihole')
  }}
/>

<!-- Starting a stack starts what was not running; the sidebar says so too. -->
<Story
  name="Starting a stack"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-stack-media')
    await appears(canvas, 'containers-stack-start')
    await userEvent.click(canvas.getByTestId('containers-stack-start'))
    await says(canvas, 'containers-summary', '2 of 2 containers running')
    await appears(canvas, 'containers-item-media-transcoder-1-stop')
  }}
/>

<!-- A container is stopped from its row, and started again. -->
<Story
  name="Stopping and starting a container"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await appears(canvas, 'containers-item-pihole-stop')
    await userEvent.click(canvas.getByTestId('containers-item-pihole-stop'))
    await appears(canvas, 'containers-item-pihole-start')
    await says(canvas, 'containers-summary', '3 of 6 containers running')
    await userEvent.click(canvas.getByTestId('containers-item-pihole-start'))
    await appears(canvas, 'containers-item-pihole-stop')
  }}
/>

<!-- A container looked at: its logs, in place of the list. -->
<Story
  name="Logs of a container"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await inspect(canvas, userEvent, 'blog-db-1')
    await says(canvas, 'containers-inspector-name', 'blog-db-1')
    await says(canvas, 'containers-logs', 'blog-db-1 started')
  }}
/>

<Story
  name="Details of a container"
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await inspect(canvas, userEvent, 'blog-db-1')
    // the kit's segmented control holds its segments in its shadow tree
    await userEvent.click(canvas.getByTestId('containers-inspector-tab').shadowRoot!.querySelector('[data-testid="containers-inspector-tab-details"]')!)
    await says(canvas, 'containers-details', 'docker-entrypoint.sh postgres')
  }}
/>

<!-- Removing the container that is looked at goes back to the list, without it. -->
<Story
  name="Removing a container"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await inspect(canvas, userEvent, 'scratch')
    await userEvent.click(canvas.getByTestId('containers-inspector-remove'))
    await appears(canvas, 'containers-inspector-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('containers-inspector-confirm'), 'action'))
    await appears(canvas, 'containers-items')
    await gone(canvas, 'containers-item-scratch')
    await says(canvas, 'containers-summary', '4 of 5 containers running')
  }}
/>

<!-- Taking a stack down leaves the other containers, and shows them all. -->
<Story
  name="Removing a stack"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-stack-blog')
    await appears(canvas, 'containers-stack-remove')
    await userEvent.click(canvas.getByTestId('containers-stack-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-stack-confirm'), 'action'))
    await gone(canvas, 'containers-stack-blog')
    await appears(canvas, 'containers-stack-media')
  }}
/>

<!-- Docker refuses (a port is taken): its own sentence is shown, and the container stays as it was. -->
<Story
  name="Docker refuses"
  args={{ backend: () => createFakeBackend({ refusing: ['scratch'] }) }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await appears(canvas, 'containers-item-scratch-start')
    await userEvent.click(canvas.getByTestId('containers-item-scratch-start'))
    await says(canvas, 'containers-failure', 'port is already allocated')
    await appears(canvas, 'containers-item-scratch-start')
  }}
/>

<!-- A Docker with nothing in it. -->
<Story
  name="No containers"
  args={{ backend: () => createFakeBackend({ containers: [] }) }}
  play={async ({ canvas, userEvent }) => {
    // the note is with the stacks, which a narrow window shows first
    await appears(canvas, 'containers-no-stacks')
    await show(canvas, userEvent, 'containers-scope-all')
    await appears(canvas, 'containers-empty')
  }}
/>

<!-- Containers, none of them Compose's. -->
<Story
  name="No stacks"
  args={{ backend: () => createFakeBackend({ containers: [container('pihole', 'pihole/pihole:2024.07.0'), container('scratch', 'alpine:3.20', 'exited')] }) }}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await says(canvas, 'containers-summary', '1 of 2 containers running')
  }}
/>

<!-- Docker is not running: the window says so in the middle, whichever column would be in front, and offers the way on. -->
<Story
  name="Docker is not running"
  args={{ backend: () => createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'containers-down')
    await expect(canvas.getByTestId('containers-down')).toHaveProperty('message', 'Docker is not running on this machine, or cannot be reached.')
    await appears(canvas, 'containers-host-connect')
  }}
/>

<!-- Another Docker is chosen from the sidebar; nothing listens there, and this machine's own is chosen again. -->
<Story
  name="Choosing another Docker"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await show(canvas, userEvent, 'containers-scope-all')
    await appears(canvas, 'containers-items')
    // in a narrow window the gear is with the stacks
    if (canvas.queryByTestId('containers-back')) await userEvent.click(canvas.getByTestId('containers-back'))
    await userEvent.click(canvas.getByTestId('containers-choose'))
    await userEvent.type(input(canvas, 'containers-host-input'), 'tcp://nas.local:2375')
    await userEvent.click(canvas.getByTestId('containers-host-connect'))
    await waitFor(() =>
      expect(canvas.getByTestId('containers-down')).toHaveProperty('message', 'Docker cannot be reached at tcp://nas.local:2375.')
    )
    // empty the field: select what it holds and delete it (the input is in the kit field's shadow tree)
    await userEvent.type(input(canvas, 'containers-host-input'), '{Backspace}', { initialSelectionStart: 0, initialSelectionEnd: 1000 })
    await userEvent.click(canvas.getByTestId('containers-host-connect'))
    await gone(canvas, 'containers-down')
    await appears(canvas, 'containers-stack-blog')
  }}
/>

<!-- The host itself does not answer: the reason is shown, and nothing is said about Docker. -->
<Story
  name="The host does not answer"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'containers-failure', 'The host is not answering.')
    await expect(canvas.queryByTestId('containers-down')).toBeNull()
  }}
/>
