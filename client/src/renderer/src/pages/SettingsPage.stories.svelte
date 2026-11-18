<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import SettingsPage from './SettingsPage.svelte'
  import { apps } from '../lib/apps.svelte'

  const app = apps.settings

  /** Settings: every pane, and the destructive-action alert. */
  const { Story } = defineMeta({
    title: 'Apps/Settings',
    component: SettingsPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <SettingsPage />
  </AppWindow>
{/snippet}

<Story name="Display & Appearance" />

<Story
  name="General"
  play={async ({ canvas, userEvent }) =>
    userEvent.click(canvas.getByTestId('settings-pane-general'))}
/>

<Story
  name="Agents"
  play={async ({ canvas, userEvent }) =>
    userEvent.click(canvas.getByTestId('settings-pane-agents'))}
/>

<Story
  name="Notifications off"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-notifications'))
    await userEvent.click(canvas.getByTestId('settings-allow-notifications'))
    await expect(canvas.getByTestId('settings-sounds')).toBeDisabled()
  }}
/>

<Story
  name="Privacy & Security"
  play={async ({ canvas, userEvent }) =>
    userEvent.click(canvas.getByTestId('settings-pane-privacy'))}
/>

<Story
  name="Reset agents alert"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-agents'))
    await userEvent.click(canvas.getByTestId('settings-reset-agents'))
    await expect(canvas.getByTestId('settings-reset-alert')).toBeVisible()
  }}
/>

<Story
  name="Search"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('settings-search'), 'priv')
    await expect(canvas.queryByTestId('settings-pane-general')).toBeNull()
  }}
/>
