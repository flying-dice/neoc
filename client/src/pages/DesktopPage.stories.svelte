<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import App from '../App.svelte'
  import { expect, waitFor } from 'storybook/test'
  import { host } from '../lib/host.svelte'
  import { windowManager } from '../lib/windowManager.svelte'
  import { createFakeHost, FAKE_AGENTS_APP, FAKE_CALCULATOR } from '../shell/fakeHost'

  const reset = (): void => {
    for (const win of [...windowManager.windows]) windowManager.close(win.id)
  }

  /** On a compact screen the app opened at launch (Discover) has the whole screen above the Dock, which stays up. */
  const presentedFullScreen = async (
    canvasElement: HTMLElement,
    canvas: { findByTestId: (id: string) => Promise<HTMLElement> },
    width: number
  ): Promise<void> => {
    const win = await canvas.findByTestId('window-discover')
    await waitFor(() => expect(win.classList.contains('fullscreen')).toBe(true))
    const screen = canvasElement.querySelector<HTMLElement>('[data-testid="desktop-screen"]')
    const left = screen?.getBoundingClientRect().left ?? 0
    const box = win.getBoundingClientRect()
    await expect(box.left - left).toBe(8)
    await expect(box.right - left).toBe(width - 8) // never past the right edge
    const dock = (await canvas.findByTestId('dock')).getBoundingClientRect()
    await expect(box.bottom).toBeLessThanOrEqual(dock.top)
  }

  /** The whole desktop shell (status bar, strip, windows, Dock) as App.svelte composes it. */
  const { Story } = defineMeta({
    title: 'Desktop/Shell',
    component: App,
    parameters: { layout: 'fullscreen' },
    beforeEach: reset,
    render: template
  })
</script>

{#snippet template(args: { width?: number; height?: number })}
  <div
    data-testid="desktop-screen"
    style:height={args.height ? `${args.height}px` : '100vh'}
    style:width={args.width ? `${args.width}px` : undefined}
  >
    <App />
  </div>
{/snippet}

<Story name="Landing" />

<!-- A host without the Agents app: the widget lists the agents as text, not as buttons that would open nothing. -->
<Story
  name="Agents widget without the Agents app"
  play={async ({ canvas }) => {
    const row = await canvas.findByTestId('widget-agent-activity-agent-scripted')
    await expect(row.tagName).toBe('DIV')
    await expect(canvas.queryByRole('button', { name: /Scripted agent/ })).toBeNull()
    await expect(canvas.queryByTestId('dock-agents')).toBeNull()
  }}
/>

<!-- With the Agents app running, a row opens it, and the Dock has its icon. -->
<Story
  name="Agents widget with the Agents app"
  beforeEach={() => {
    host.useFake(createFakeHost({ apps: [FAKE_CALCULATOR, FAKE_AGENTS_APP] }), [
      FAKE_CALCULATOR,
      FAKE_AGENTS_APP
    ])
    return () => host.useFake(createFakeHost(), [FAKE_CALCULATOR])
  }}
  play={async ({ canvas }) => {
    const row = await canvas.findByTestId('widget-agent-activity-agent-scripted')
    await expect(row.tagName).toBe('BUTTON')
    await expect(await canvas.findByTestId('dock-agents')).toBeVisible()
  }}
/>

<Story
  name="Split View"
  play={async () => {
    windowManager.open('files')
    const files = windowManager.windows.find((w) => w.appId === 'files')!
    windowManager.setLayout(files.id, 'split-left')
    windowManager.open('settings')
  }}
/>

<!-- A phone held upright (390 px wide): one app at a time, full screen above the Dock, nothing past the right edge. -->
<Story
  name="Phone width"
  args={{ width: 390, height: 844 }}
  play={({ canvasElement, canvas }) => presentedFullScreen(canvasElement, canvas, 390)}
/>

<!-- The same phone on its side: a short screen is compact too, as on iOS. -->
<Story
  name="Phone width, on its side"
  args={{ width: 844, height: 390 }}
  play={({ canvasElement, canvas }) => presentedFullScreen(canvasElement, canvas, 844)}
/>
