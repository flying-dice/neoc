<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import App from '../App.svelte'
  import { windowManager } from '../lib/windowManager.svelte'

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

<Story
  name="Split View"
  play={async () => {
    windowManager.open('files')
    const files = windowManager.windows.find((w) => w.appId === 'files')!
    windowManager.setLayout(files.id, 'split-left')
    windowManager.open('settings')
  }}
/>
