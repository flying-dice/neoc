<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import App from '../App.svelte'
  import { expect, waitFor } from 'storybook/test'
  import { windowManager } from '../lib/windowManager.svelte'

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
