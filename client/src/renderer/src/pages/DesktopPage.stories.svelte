<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import App from '../App.svelte'
  import { expect } from 'storybook/test'
  import { host } from '../lib/host.svelte'
  import { windowManager } from '../lib/windowManager.svelte'
  import { createFakeHost, FAKE_AGENTS_APP, FAKE_CALCULATOR } from '../shell/fakeHost'

  const reset = (): void => {
    for (const win of [...windowManager.windows]) windowManager.close(win.id)
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

{#snippet template()}
  <div style="height: 100vh">
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
