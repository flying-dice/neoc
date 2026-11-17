<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import TerminalPage from './TerminalPage.svelte'
  import { apps } from '../lib/apps'

  const app = apps.terminal

  /** Terminal: agent shell output (always dark). */
  const { Story } = defineMeta({
    title: 'Apps/Terminal',
    component: TerminalPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <TerminalPage />
  </AppWindow>
{/snippet}

<Story name="Status" />
