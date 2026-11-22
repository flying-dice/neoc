<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import SettingsPage from './SettingsPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import { createFakeHost, FAKE_CALCULATOR } from '../shell/fakeHost'

  const app = apps.settings

  /** Settings: every pane, the paired devices, and the revoke alert. */
  const { Story } = defineMeta({
    title: 'Apps/Settings',
    component: SettingsPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    beforeEach: () => host.useFake(createFakeHost(), [FAKE_CALCULATOR]),
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
  name="Search"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('settings-search'), 'priv')
    await expect(canvas.queryByTestId('settings-pane-general')).toBeNull()
  }}
/>

<Story
  name="Devices"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await expect(await canvas.findByTestId('settings-device-d1')).toHaveTextContent('This device')
    await expect(canvas.queryByTestId('settings-device-d1-revoke')).toBeNull()
    await expect(canvas.getByTestId('settings-device-d2-revoke')).toBeVisible()
  }}
/>

<Story
  name="Revoke device alert"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await userEvent.click(await canvas.findByTestId('settings-device-d2-revoke'))
    await expect(canvas.getByTestId('settings-device-revoke-alert')).toBeVisible()
  }}
/>

<Story
  name="Device revoked"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await userEvent.click(await canvas.findByTestId('settings-device-d2-revoke'))
    await userEvent.click(canvas.getByTestId('settings-device-revoke-alert-action-revoke'))
    await expect(canvas.queryByTestId('settings-device-d2')).toBeNull()
    await expect(canvas.getByTestId('settings-device-d3')).toBeVisible()
  }}
/>
