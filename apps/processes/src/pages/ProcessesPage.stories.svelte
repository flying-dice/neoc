<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import definition from '../../openrpc.json'
  import ProcessesPage from './ProcessesPage.svelte'
  import { PROGRAMS, createFailingBackend, createFakeBackend, type FakeBackend, type FakeOptions } from '../lib/fakeApi'
  import { Processes } from '../lib/processes.svelte'

  // the window the contract asks for, and the smallest it allows
  const app = definition['x-app']

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host or a real machine. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Processes app in its window, on a fake machine: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Processes',
    component: ProcessesPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => lab() },
    argTypes: { backend: { control: false } },
    render: template
  })

  /**
   * A small home lab: three processes, so that every row fits in the window of the contract (a row that is cut off at
   * the bottom of a scrolling list is read by axe as text that is not there).
   */
  const lab = (options: FakeOptions = {}): FakeBackend => createFakeBackend({ programs: PROGRAMS.filter((p) => [1, 702, 812].includes(p.pid)), ...options })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  /** The kit's controls hold the real one in their shadow tree. */
  const inner = (canvas: Canvas, id: string, selector: string): HTMLElement => canvas.getByTestId(id).shadowRoot!.querySelector<HTMLElement>(selector)!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const gone = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.queryByTestId(id)).toBeNull())
  const says = (canvas: Canvas, id: string, text: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toHaveTextContent(text))
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const processes = new Processes(backend.rpc, backend.events)}
  <!-- a story reads the machine once: a window that read it on a timer would change under the story's checks -->
  {@const _ = processes.every(0)}
  <AppWindow title={definition.info.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <ProcessesPage {processes} />
  </AppWindow>
{/snippet}

<!-- A home lab: the busiest process first, and nothing chosen yet. -->
<Story
  name="A home lab"
  play={async ({ canvas }) => {
    await appears(canvas, 'processes-row-812')
    await says(canvas, 'processes-tile-cpu-value', '—')
    await appears(canvas, 'processes-none')
  }}
/>

<Story name="In the smallest window" args={{ width: app.window.minW, height: app.window.minH }} />

<!-- A process is picked: its details, who runs it, and what it is next to. -->
<Story
  name="Looking at a process"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-812')
    await userEvent.click(canvas.getByTestId('processes-row-812'))
    await says(canvas, 'processes-detail-name', 'postgres')
    await says(canvas, 'processes-detail-state', 'PID 812 · Running')
    await waitFor(() => expect(canvas.getByTestId('processes-detail-user')).toHaveProperty('value', 'postgres'))
    await appears(canvas, 'processes-parent')
  }}
/>

<Story
  name="Searching"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-search')
    await userEvent.type(inner(canvas, 'processes-search', 'input'), 'postgres')
    await gone(canvas, 'processes-row-702')
    await appears(canvas, 'processes-row-812')
  }}
/>

<Story
  name="No process matches"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-search')
    await userEvent.type(inner(canvas, 'processes-search', 'input'), 'zzz')
    await waitFor(() => expect(canvas.getByTestId('processes-empty')).toHaveProperty('message', 'Nothing matches “zzz”. Check the name or PID.'))
  }}
/>

<!-- Terminate asks first: the window does not send anything until the answer. -->
<Story
  name="Asked before ending"
  tags={['keep-focus']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-812')
    await userEvent.click(canvas.getByTestId('processes-row-812'))
    await appears(canvas, 'processes-force')
    await waitFor(() => expect(canvas.getByTestId('processes-force')).not.toHaveAttribute('disabled'))
    await userEvent.click(canvas.getByTestId('processes-force'))
    // the kit's alert holds its words in its shadow tree
    const alert = (): Element | null => alertDialog(canvas.getByTestId('processes-confirm'))
    await waitFor(() => expect(alert()).toHaveTextContent('Force Terminate “postgres”?'))
    await expect(alert()).toHaveTextContent('It stops at once and cannot save its work.')
  }}
/>

<Story
  name="Ending a process"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-812')
    await userEvent.click(canvas.getByTestId('processes-row-812'))
    await waitFor(() => expect(canvas.getByTestId('processes-terminate')).not.toHaveAttribute('disabled'))
    await userEvent.click(canvas.getByTestId('processes-terminate'))
    await appears(canvas, 'processes-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('processes-confirm'), 'cancel'))
    await gone(canvas, 'processes-confirm')
    await expect(canvas.queryByTestId('processes-notice')).toBeNull()
    await userEvent.click(canvas.getByTestId('processes-terminate'))
    await appears(canvas, 'processes-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('processes-confirm'), 'action'))
    await says(canvas, 'processes-notice', 'Asked postgres to quit.')
    await says(canvas, 'processes-detail-state', 'Quit')
    await appears(canvas, 'processes-quit')
  }}
/>

<!-- The system says no: it is said at the process, with what to do, and nothing was stopped. -->
<Story
  name="The system refuses"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-702')
    await userEvent.click(canvas.getByTestId('processes-row-702'))
    await waitFor(() => expect(canvas.getByTestId('processes-force')).not.toHaveAttribute('disabled'))
    await userEvent.click(canvas.getByTestId('processes-force'))
    await appears(canvas, 'processes-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('processes-confirm'), 'action'))
    await says(canvas, 'processes-refused', 'Can’t Terminate sshd')
    await expect(canvas.getByTestId('processes-refused')).toHaveTextContent('ask an administrator')
    await expect(canvas.queryByTestId('processes-failure')).toBeNull()
  }}
/>

<!-- The system would not let Processes hold it for ending: it can still be looked at, and the window says why it cannot be ended. -->
<Story
  name="A process that cannot be ended"
  args={{ backend: () => lab({ programs: PROGRAMS.filter((p) => [1, 702, 812].includes(p.pid)).map((p) => (p.pid === 702 ? { ...p, blocked: 'permission' as const } : p)) }) }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-702')
    await userEvent.click(canvas.getByTestId('processes-row-702'))
    await says(canvas, 'processes-detail-name', 'sshd')
    await appears(canvas, 'processes-unavailable')
    await expect(canvas.getByTestId('processes-unavailable')).toHaveTextContent('ask an administrator')
    await expect(canvas.getByTestId('processes-detail-memory')).toBeVisible()
    await expect(canvas.getByTestId('processes-force')).toHaveAttribute('disabled')
  }}
/>

<!-- A system with no way to ask a program to quit (Windows): only the forced stop is offered. -->
<Story
  name="No request to quit"
  args={{ backend: () => lab({ cannotTerminate: true }) }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-row-812')
    await userEvent.click(canvas.getByTestId('processes-row-812'))
    await appears(canvas, 'processes-force')
    await expect(canvas.queryByTestId('processes-terminate')).toBeNull()
  }}
/>

<!-- The machine as a whole. -->
<Story
  name="The machine as a whole"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'processes-view-overview')
    await userEvent.click(canvas.getByTestId('processes-view-overview'))
    await appears(canvas, 'processes-overview')
    await waitFor(() => expect(canvas.getByTestId('processes-uptime')).toHaveProperty('value', '3 d 4 h'))
  }}
/>

<Story
  name="The host is not answering"
  args={{ backend: () => createFailingBackend('The host is not answering.') }}
  play={async ({ canvas }) => {
    await says(canvas, 'processes-failure', 'The host is not answering.')
  }}
/>
