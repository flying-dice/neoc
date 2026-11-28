<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import TelemetryPage from './TelemetryPage.svelte'
  import { FAILED_TRACE, RENDER_TRACE, createFakeBackend, rpcError, type FakeBackend } from '../lib/fakeApi'
  import { Telemetry } from '../lib/telemetry.svelte'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The host page and backend the story's window talks to: a fake in the page. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  /** The Telemetry app in its window, over recorded traces held in the page: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Telemetry',
    component: TelemetryPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
</script>

{#snippet template(args: Args)}
  {@const backend = args.backend()}
  <!-- the failing render is never run: a story's page must not throw -->
  <!-- and a copy goes nowhere real: the clipboard is a note of what was written -->
  {@const telemetry = new Telemetry(backend.reader, backend.rpc, { later: () => {}, clipboard: { write: async () => {} }, after: () => {} })}
  <AppWindow title={app.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <!-- a story reads once: a window that read on a timer would change under the story's checks -->
    <TelemetryPage {telemetry} every={0} />
  </AppWindow>
{/snippet}

<!-- Two traces, none open yet. -->
<Story
  name="Two traces"
  play={async ({ canvas }) => {
    await appears(canvas, `telemetry-trace-${FAILED_TRACE}`)
    await appears(canvas, 'telemetry-none')
  }}
/>

<!-- From the list to the trace of the Services window that failed to draw (NEOC-520), and its error log. -->
<Story
  name="Following a render failure"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, `telemetry-trace-${RENDER_TRACE}`)
    await userEvent.click(canvas.getByTestId(`telemetry-trace-${RENDER_TRACE}`))
    await appears(canvas, 'telemetry-span-2222222222222222')
    await userEvent.click(canvas.getByTestId('telemetry-trace-log-1'))
    await appears(canvas, 'telemetry-log-details')
    await expect((canvas.getByTestId('telemetry-span-details') as HTMLElement & { header: string }).header).toBe('Span · rpc overview')
  }}
/>

<!-- Copy on the trace puts everything about it on the clipboard, and the button says so. -->
<Story
  name="Copying a trace"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId(`telemetry-trace-${RENDER_TRACE}`))
    await appears(canvas, 'telemetry-span-2222222222222222')
    await userEvent.click(canvas.getByTestId('telemetry-trace-copy'))
    await waitFor(() => expect(canvas.getByTestId('telemetry-trace-copy-status')).toHaveTextContent('on the clipboard'))
  }}
/>

<Story
  name="The logs"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'telemetry-view-logs')
    await userEvent.click(canvas.getByTestId('telemetry-view-logs'))
    await appears(canvas, 'telemetry-log-0')
    await userEvent.click(canvas.getByTestId('telemetry-log-0'))
    await appears(canvas, 'telemetry-log-trace')
  }}
/>

<Story name="Nothing recorded yet" args={{ backend: () => createFakeBackend({ spans: [], logs: [] }) }} />

<Story name="Loading" args={{ backend: () => createFakeBackend({ hang: true }) }} />

<!-- The host page does not offer telemetry to this frame. -->
<Story
  name="Unavailable"
  args={{ backend: () => createFakeBackend({ failWith: rpcError(-32601, 'This host does not offer vos/telemetry/query') }) }}
  play={async ({ canvas }) => appears(canvas, 'telemetry-unavailable')}
/>

<Story
  name="The host is away"
  args={{ backend: () => createFakeBackend({ failWith: rpcError(-32000, 'The connection to the host was lost. It reconnects on its own.') }) }}
  play={async ({ canvas }) => appears(canvas, 'telemetry-failure')}
/>

<!-- The smallest window the contract allows shows one column. -->
<Story name="Narrow" args={{ width: app.window.minW, height: app.window.minH }} />
