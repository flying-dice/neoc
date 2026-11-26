<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import TerminalPage from './TerminalPage.svelte'
  import { Terminal } from '../lib/terminal.svelte'
  import { createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's session talks to: a script in the page, never a host and never a shell. */
    backend: FakeBackend
  }

  /** The Terminal in its window, on a fake backend (a scripted shell): every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Terminal',
    component: TerminalPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: createFakeBackend() },
    argTypes: { backend: { control: false } },
    // a backend of its own for each story: sessions do not carry over
    beforeEach: ({ args }) => {
      args.backend = createFakeBackend(args.backend === refusing ? { refuse: 'The shell could not be started.' } : {})
    },
    render: template
  })

  /** Marks the story whose backend starts no shell. */
  const refusing = createFakeBackend({ refuse: 'The shell could not be started.' })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  const page = (canvas: Canvas): HTMLElement => canvas.getByTestId('terminal-app')
  const screen = (canvas: Canvas): HTMLElement => canvas.getByTestId('terminal-screen')
  /** The session is running and the shell's prompt is on the screen. */
  async function prompted(canvas: Canvas): Promise<void> {
    await waitFor(() => expect(page(canvas)).toHaveAttribute('data-phase', 'running'))
    await waitFor(() => expect(screen(canvas)).toHaveTextContent('$'))
  }
</script>

{#snippet template(args: Args)}
  <!-- one session per story, as each window has one -->
  {@const terminal = new Terminal(args.backend.rpc, args.backend.events)}
  <AppWindow title={app.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <TerminalPage {terminal} />
  </AppWindow>
{/snippet}

<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await prompted(canvas)
    await expect(canvas.queryByTestId('terminal-bar')).toBeNull()
  }}
/>

<!-- What is typed goes to the shell, and what it prints is drawn. -->
<Story
  name="A command and its output"
  play={async ({ canvas, userEvent }) => {
    await prompted(canvas)
    await userEvent.keyboard('echo hello world{Enter}')
    await waitFor(() => expect(screen(canvas)).toHaveTextContent('$ echo hello world'))
    await waitFor(() => expect(screen(canvas).textContent).toMatch(/hello world.*hello world/s))
  }}
/>

<!-- The shell ended by itself: the output stays, a line says so, and New Session starts another on an empty screen. -->
<Story
  name="The shell exited"
  play={async ({ canvas, userEvent }) => {
    await prompted(canvas)
    await userEvent.keyboard('exit 3{Enter}')
    await waitFor(() => expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('The shell exited with code 3.'))
    await expect(canvas.getByTestId('terminal-bar-action')).toHaveTextContent('New Session')
  }}
/>

<Story
  name="A new session after the exit"
  tags={['play-only']}
  play={async ({ canvas, userEvent, args }) => {
    await prompted(canvas)
    await userEvent.keyboard('echo first{Enter}exit{Enter}')
    await waitFor(() => expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('The shell exited.'))
    await userEvent.click(canvas.getByTestId('terminal-bar-action'))
    await prompted(canvas)
    await expect(canvas.queryByTestId('terminal-bar')).toBeNull()
    await expect(screen(canvas)).not.toHaveTextContent('first')
    await expect(args.backend.open()).toHaveLength(1)
    // and it has the keyboard again
    await userEvent.keyboard('echo second{Enter}')
    await waitFor(() => expect(screen(canvas).textContent).toMatch(/second.*second/s))
  }}
/>

<!-- No shell could be started: the reason, and a way to try again. -->
<Story
  name="The shell could not be started"
  args={{ backend: refusing }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('The shell could not be started.'))
    await expect(canvas.getByTestId('terminal-bar-action')).toHaveTextContent('Try Again')
  }}
/>

<!-- The host connection is away: a line says so, and what is typed waits. When it is back the session goes on, with
     what the shell printed meanwhile. -->
<Story
  name="The connection is away"
  play={async ({ canvas, userEvent, args }) => {
    await prompted(canvas)
    args.backend.offline(true)
    await userEvent.keyboard('echo waited{Enter}')
    await waitFor(() => expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('The host is not answering.'))
    await expect(canvas.queryByTestId('terminal-bar-action')).toBeNull()
  }}
/>

<Story
  name="The connection is back"
  tags={['play-only']}
  play={async ({ canvas, userEvent, args }) => {
    await prompted(canvas)
    args.backend.offline(true)
    await userEvent.keyboard('echo waited{Enter}')
    await waitFor(() => expect(canvas.getByTestId('terminal-bar')).toBeVisible())
    args.backend.offline(false)
    args.backend.emit('host:reconnected', null)
    await waitFor(() => expect(canvas.queryByTestId('terminal-bar')).toBeNull())
    await waitFor(() => expect(screen(canvas).textContent).toMatch(/waited.*waited/s))
  }}
/>

<!-- The backend ended the session while the window was away for too long. -->
<Story
  name="The session has ended"
  play={async ({ canvas, args }) => {
    await prompted(canvas)
    args.backend.forget()
    args.backend.emit('host:reconnected', null)
    await waitFor(() => expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('This session has ended'))
    await expect(canvas.getByTestId('terminal-bar-action')).toHaveTextContent('New Session')
  }}
/>

<!-- The smallest window the manifest allows: the emulator has fewer columns, and the shell is told. -->
<Story
  name="The smallest window"
  args={{ width: app.window.minW, height: app.window.minH }}
  play={async ({ canvas, userEvent }) => {
    await prompted(canvas)
    await userEvent.keyboard('stty size{Enter}')
    await waitFor(() => expect(screen(canvas).textContent).toMatch(/\d+ \d+/))
    await expect(screen(canvas).scrollWidth).toBeLessThanOrEqual(screen(canvas).clientWidth)
  }}
/>
