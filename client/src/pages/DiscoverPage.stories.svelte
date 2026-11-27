<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import DiscoverPage from './DiscoverPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import { createFakeHost, FAKE_CALCULATOR, type FakeUpdate } from '../shell/fakeHost'
  import type { AppSummary, HostApi } from '../shell/hostApi'

  const app = apps.discover

  /** The host a story talks to: the Calculator installed, Notes offered by the registry. */
  const withHost =
    (installed: AppSummary[] = [FAKE_CALCULATOR], registryErrors: string[] = []) =>
    () =>
      host.useFake(createFakeHost({ apps: installed, registryErrors }), installed)

  /** The Calculator installed at 0.1.0 and offered at 0.2.0, which asks for the camera. */
  const withUpdate = (updateError?: string) => (): void => {
    const updates: Record<string, FakeUpdate> = {
      calculator: { version: '0.2.0', permissions: ['camera'] }
    }
    const installed = [{ ...FAKE_CALCULATOR }]
    host.useFake(createFakeHost({ apps: installed, updates, updateError }), installed)
  }

  /** A host from another version: its answers to the shell's own calls are not the shapes this client knows. */
  const withUnknownShapes = async (): Promise<void> => {
    const base = createFakeHost()
    const api: HostApi = {
      call: (method: string, params: Record<string, unknown> = {}): Promise<unknown> =>
        method === 'app.list'
          ? Promise.resolve({ apps: null })
          : method === 'app.available'
            ? Promise.resolve({ apps: 5, errors: 'none' })
            : base.call(method, params),
      onNotification: (listener) => base.onNotification(listener)
    }
    host.useFake(api, [FAKE_CALCULATOR])
    await host.refresh()
  }

  const notesDisabled: AppSummary = {
    id: 'notes',
    name: 'Notes',
    version: '0.2.0',
    state: 'disabled',
    icon: 'doc',
    tint: 'yellow',
    description: 'Plain notes your agents can read and append to.',
    source: 'this computer',
    permissions: []
  }

  /** Discover: the landing window, and the place apps are found, installed and managed. */
  const { Story } = defineMeta({
    title: 'Apps/Discover',
    component: DiscoverPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    beforeEach: withHost(),
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <DiscoverPage />
  </AppWindow>
{/snippet}

<Story
  name="Featured"
  play={async ({ canvas }) => {
    // a listing's GET / OPEN, Disable and Remove are targets a finger can hit (44 pt)
    const ids = [
      'discover-app-calculator-get',
      'discover-app-calculator-toggle',
      'discover-app-calculator-remove'
    ]
    for (const id of ids) {
      const button = await canvas.findByTestId(id)
      await expect(button.getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
    }
  }}
/>

<Story name="Narrow window" args={{ width: 380, height: 640 }} />

<Story
  name="No search results"
  play={async ({ canvas, userEvent }) => {
    // a window too narrow for the sidebar shows the page alone: there is no field to search in
    if (!canvas.queryByTestId('discover-search')) return
    await userEvent.type(canvas.getByTestId('discover-search'), 'spreadsheet')
    await expect(await canvas.findByText('No apps match “spreadsheet”.')).toBeVisible()
  }}
/>

<Story
  name="Apps from the registry"
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('discover-app-notes-get')).toHaveTextContent('GET')
    await expect(canvas.getByTestId('discover-app-calculator-get')).toHaveTextContent('OPEN')
  }}
/>

<Story
  name="Get an app"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('discover-app-notes-get'))
    await expect(await canvas.findByTestId('discover-app-notes-remove')).toBeVisible()
    await expect(canvas.getByTestId('discover-app-notes-get')).toHaveTextContent('OPEN')
  }}
/>

<Story
  name="Disabled app"
  beforeEach={withHost([FAKE_CALCULATOR, notesDisabled])}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('discover-app-notes-state')).toHaveTextContent(
      'Disabled'
    )
    await expect(canvas.getByTestId('discover-app-notes-get')).toHaveTextContent('ENABLE')
    await expect(canvas.queryByTestId('discover-app-notes-toggle')).toBeNull()
  }}
/>

<Story
  name="Registry unreachable"
  beforeEach={withHost(
    [FAKE_CALCULATOR],
    ['file:///gone/registry.json: not found: file:///gone/registry.json']
  )}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('discover-registry-error')).toBeVisible()
  }}
/>

<!-- The sidebar chooses what the page shows: the front page (the welcome, the apps, the numbers) or the apps alone. -->
<Story
  name="The apps alone"
  beforeEach={withHost([FAKE_CALCULATOR])}
  play={async ({ canvas, userEvent }) => {
    await expect(await canvas.findByTestId('discover-hero')).toBeVisible()
    // a window too narrow for the sidebar shows the page alone: there is nothing to choose a section with
    if (!canvas.queryByTestId('discover-section-apps')) return
    await userEvent.click(canvas.getByTestId('discover-section-apps'))
    await expect(canvas.queryByTestId('discover-hero')).toBeNull()
    await expect(canvas.queryByTestId('discover-information')).toBeNull()
    await expect(canvas.getByTestId('discover-apps')).toBeVisible()
  }}
/>

<!-- The numbers are the host's: apps running. No made-up badge on Updates. -->
<Story
  name="Counts come from the host"
  beforeEach={withHost([FAKE_CALCULATOR, notesDisabled])}
  play={async ({ canvas }) => {
    await expect(await canvas.findByText('1 of 2')).toBeVisible()
    await expect(canvas.queryByTestId('discover-section-updates')).toBeNull() // no made-up badge or section
  }}
/>

<!-- A host answering in an unknown shape is an error on the page, not a crash of the desktop. -->
<Story
  name="Host answers in an unknown shape"
  beforeEach={withUnknownShapes}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('discover-apps-list-error')).toHaveTextContent(
      'unexpected answer to “app.list”'
    )
    await expect(await canvas.findByTestId('discover-apps-error')).toHaveTextContent(
      'unexpected answer to “app.available”'
    )
  }}
/>

<!-- The same registry problem reported twice shows once (the page keys problems by their text). -->
<Story
  name="Repeated registry problem"
  beforeEach={withHost(
    [FAKE_CALCULATOR],
    ['registry down', 'registry down', 'other registry down']
  )}
  play={async ({ canvas }) => {
    await canvas.findAllByTestId('discover-registry-error')
    await expect(canvas.getAllByTestId('discover-registry-error')).toHaveLength(2)
  }}
/>

<!-- An installed app with a newer version: UPDATE says the version change and asks first, with what the new version wants. -->
<Story
  name="Update an app"
  beforeEach={withUpdate()}
  play={async ({ canvas, userEvent }) => {
    const listing = await canvas.findByTestId('discover-app-calculator')
    await expect(listing).toHaveTextContent('0.1.0 → 0.2.0')
    await userEvent.click(canvas.getByTestId('discover-app-calculator-update'))
    const alert = await canvas.findByTestId('discover-update-alert')
    await expect(alert).toHaveTextContent('Update Calculator?')
    await expect(alert).toHaveTextContent('camera (new)')
    await userEvent.click(canvas.getByTestId('discover-update-alert-action-update'))
    // the listing says the version it now has (the text is in it more than once: its row and its name for a reader)
    await waitFor(() => expect(canvas.getByTestId('discover-app-calculator')).toHaveTextContent('0.2.0 · this computer'))
    await expect(canvas.getByTestId('discover-app-calculator')).not.toHaveTextContent('→')
    await expect(canvas.queryByTestId('discover-app-calculator-update')).toBeNull()
  }}
/>

<!-- Cancel changes nothing. -->
<Story
  name="Update declined"
  beforeEach={withUpdate()}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('discover-app-calculator-update'))
    await userEvent.click(await canvas.findByTestId('discover-update-alert-action-cancel'))
    await expect(canvas.queryByTestId('discover-update-alert')).toBeNull()
    await expect(canvas.getByTestId('discover-app-calculator')).toHaveTextContent('0.1.0 → 0.2.0')
  }}
/>

<!-- An update the host refuses says why, in words, and the installed version stays. -->
<Story
  name="Update fails"
  beforeEach={withUpdate('the app ref changed since it was previewed; preview it again')}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('discover-app-calculator-update'))
    await userEvent.click(await canvas.findByTestId('discover-update-alert-action-update'))
    await expect(await canvas.findByTestId('discover-apps-error')).toHaveTextContent(
      'the app ref changed since it was previewed'
    )
    await expect(canvas.getByTestId('discover-app-calculator')).toHaveTextContent('0.1.0 → 0.2.0')
  }}
/>
