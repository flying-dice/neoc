<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import AppFrame from './AppFrame.svelte'
  import { host } from '../lib/host.svelte'
  import { HostError } from '../shell/channel'
  import { createFakeHost, FAKE_CALCULATOR } from '../shell/fakeHost'
  import type { HostApi } from '../shell/hostApi'

  const size = FAKE_CALCULATOR.window!

  /** The end of a script element, written so it does not end this script. */
  const CLOSE = '<' + '/script>'

  /** What the app's client does in these stories, in place of the host's frame document and bridge: it says it is ready,
   *  asks its backend for a greeting with `rpc`, and shows the answer, or the error the backend gave. */
  const client = (
    method: string
  ): string => `<!doctype html><html lang="en"><title>Calculator</title><body style="font:16px system-ui;margin:16px">
<p id="out">waiting</p>
<script>
  parent.postMessage({ t: 'ready' }, '*')
  window.addEventListener('message', (e) => {
    const m = e.data
    if (m.t !== 'res') document.getElementById('out').textContent = 'event ' + m.name + ': ' + m.data
    else document.getElementById('out').textContent = 'error' in m ? 'Error: ' + m.error : 'Hello, ' + m.result
  })
  parent.postMessage({ t: 'rpc', id: 1, method: ${JSON.stringify(method)}, args: null }, '*')
${CLOSE}</body></html>`

  /** The shell's host connection, answering `app.call` like the host would and keeping what it was asked. */
  let calls: Record<string, unknown>[] = []
  let notify: ((method: string, params: Record<string, unknown>) => void) | undefined

  const withHost = (answer: (params: Record<string, unknown>) => unknown) => () => {
    calls = []
    const base = createFakeHost()
    const api: HostApi = {
      call: async (method, params = {}) => {
        if (method !== 'app.call') return base.call(method, params)
        calls.push(params)
        return answer(params)
      },
      onNotification: (listener) => {
        notify = listener
        return () => (notify = undefined)
      }
    }
    host.useFake(api, [FAKE_CALCULATOR])
    return () => host.useFake(createFakeHost(), [FAKE_CALCULATOR])
  }

  /** An installed app's window when the app has a client of its own: a sandboxed frame, a bridge to the app's backend. */
  const { Story } = defineMeta({
    title: 'Shell/App frame',
    component: AppFrame,
    tags: ['app'],
    args: { width: size.w, height: size.h },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number; srcdoc?: string; patience?: number })}
  <AppWindow title={FAKE_CALCULATOR.name} {...args} minWidth={size.minW} minHeight={size.minH}>
    <AppFrame
      appId="calculator"
      name={FAKE_CALCULATOR.name}
      windowId="w1"
      srcdoc={args.srcdoc}
      patience={args.patience}
    />
  </AppWindow>
{/snippet}

<!-- The client has not said it is ready: a spinner with the app's name for a screen reader. -->
<Story
  name="Opening"
  args={{
    srcdoc: '<!doctype html><html lang="en"><title>Calculator</title><p>not mounted</p>',
    patience: 600_000
  }}
  beforeEach={withHost(() => null)}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('calculator-opening')).toBeVisible()
    await expect(canvas.getByRole('status', { name: 'Opening Calculator' })).toBeVisible()
    await expect(canvas.queryByTestId('calculator-failed')).toBeNull()
  }}
/>

<!-- The client mounted and called its backend through the bridge: the app is the frame's, the answer comes back. -->
<Story
  name="Ready"
  args={{ srcdoc: client('greet') }}
  beforeEach={withHost((p) => `${String(p.app)} (${String(p.method)})`)}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('calculator-frame')).toBeVisible())
    await waitFor(() => expect(calls).toEqual([{ app: 'calculator', method: 'greet', args: null }]))
    await expect(canvas.queryByTestId('calculator-opening')).toBeNull()
    const doc = (canvas.getByTestId('calculator-frame') as HTMLIFrameElement).contentDocument
    // a sandboxed frame has an opaque origin: the shell cannot read into it either
    await expect(doc).toBeNull()
  }}
/>

<!-- A press or a drag in the shell covers the frame, which would otherwise swallow the pointer; a drag outlives its pointer. -->
<Story
  name="Covered while something is dragged"
  args={{ srcdoc: client('greet') }}
  beforeEach={withHost(() => 'there')}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('calculator-frame')).toBeVisible())
    await expect(canvas.queryByTestId('calculator-shield')).toBeNull()
    // a press (a window being moved, the Split View divider): covered until the pointer is up
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await expect(await canvas.findByTestId('calculator-shield')).toBeInTheDocument()
    window.dispatchEvent(new PointerEvent('pointerup'))
    await waitFor(() => expect(canvas.queryByTestId('calculator-shield')).toBeNull())
    // a native drag: the browser ends the press with pointercancel at once, the cover stays until the drag ends
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    document.body.dispatchEvent(new DragEvent('dragstart', { bubbles: true }))
    window.dispatchEvent(new PointerEvent('pointercancel'))
    await expect(await canvas.findByTestId('calculator-shield')).toBeInTheDocument()
    window.dispatchEvent(new DragEvent('dragend'))
    await waitFor(() => expect(canvas.queryByTestId('calculator-shield')).toBeNull())
  }}
/>

<!-- The backend failed: its message reaches the client, which shows it (the window stays up). -->
<Story
  name="The backend fails"
  args={{ srcdoc: client('greet') }}
  beforeEach={withHost(() => Promise.reject(new HostError(-32010, 'A task needs a title.')))}
  play={async ({ canvas }) => {
    await waitFor(() => expect(calls.length).toBe(1))
    await expect(canvas.getByTestId('calculator-frame')).toBeVisible()
  }}
/>

<!-- An event the backend sent reaches the frame of its own app, and not another app's. -->
<Story
  name="Backend event"
  args={{ srcdoc: client('greet') }}
  beforeEach={withHost(() => 'there')}
  play={async ({ canvas }) => {
    await waitFor(() => expect(calls.length).toBe(1))
    notify?.('app.event', { app: 'notes', name: 'other', data: 1 })
    notify?.('app.event', { app: 'calculator', name: 'changed', data: 'yes' })
    await expect(canvas.getByTestId('calculator-frame')).toBeVisible()
  }}
/>

<!-- The client never mounts: after its patience the window says so, with a retry. -->
<Story
  name="Never starts"
  args={{
    srcdoc: '<!doctype html><html lang="en"><title>Calculator</title><p>not mounted</p>',
    patience: 200
  }}
  beforeEach={withHost(() => null)}
  play={async ({ canvas }) => {
    const failed = await canvas.findByTestId('calculator-failed')
    await expect(failed).toHaveAttribute('role', 'alert')
    await expect(failed).toHaveTextContent('Couldn’t open Calculator')
    await expect(failed).toHaveTextContent('Calculator did not start.')
    const rect = canvas.getByTestId('calculator-failed-retry').getBoundingClientRect()
    await expect(rect.height).toBeGreaterThanOrEqual(44)
  }}
/>

<!-- The client reported that it could not start (a bundle without `vosApp.mount`, a throw in `mount`). -->
<Story
  name="Reports a failure"
  args={{
    srcdoc:
      '<!doctype html><html lang="en"><title>Calculator</title><script>parent.postMessage({ t: "failed", message: "the client does not define vosApp.mount" }, "*")' +
      CLOSE
  }}
  beforeEach={withHost(() => null)}
  play={async ({ canvas }) => {
    const failed = await canvas.findByTestId('calculator-failed')
    await expect(failed).toHaveTextContent('the client does not define vosApp.mount')
  }}
/>
