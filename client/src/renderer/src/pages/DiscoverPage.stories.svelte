<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import DiscoverPage from './DiscoverPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import { createFakeHost, FAKE_CALCULATOR } from '../shell/fakeHost'
  import type { AppSummary } from '../shell/hostApi'

  const app = apps.discover

  /** The host a story talks to: the Calculator installed, Notes offered by the registry. */
  const withHost =
    (installed: AppSummary[] = [FAKE_CALCULATOR], registryErrors: string[] = []) =>
    () =>
      host.useFake(createFakeHost({ apps: installed, registryErrors }), installed)

  const notesDisabled: AppSummary = {
    id: 'notes',
    name: 'Notes',
    version: '0.2.0',
    state: 'disabled',
    icon: 'doc',
    tint: 'yellow',
    description: 'Plain notes your agents can read and append to.',
    source: 'this computer'
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

<Story name="Featured" />

<Story name="Narrow window" args={{ width: 380, height: 640 }} />

<Story
  name="Agent installed"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('discover-listing-coder-get'))
    await expect(canvas.getByTestId('discover-listing-coder-get')).toHaveTextContent(/open/i)
  }}
/>

<Story
  name="No search results"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('discover-search'), 'spreadsheet')
    await expect(canvas.queryByTestId('discover-listing-research')).toBeNull()
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
    await expect(canvas.getByTestId('discover-app-notes-toggle')).toHaveTextContent('Enable')
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

<Story
  name="Add by URL: preview"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(
      canvas.getByTestId('discover-add-url'),
      'https://apps.example.org/notes/app-ref.json'
    )
    await userEvent.click(canvas.getByTestId('discover-add-preview'))
    await expect(await canvas.findByTestId('discover-add-preview-card')).toHaveTextContent(
      'Version 0.2.0'
    )
  }}
/>

<Story
  name="Add by URL: confirm"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(
      canvas.getByTestId('discover-add-url'),
      'https://apps.example.org/notes/app-ref.json'
    )
    await userEvent.click(canvas.getByTestId('discover-add-preview'))
    await userEvent.click(await canvas.findByTestId('discover-add-install'))
    await expect(await canvas.findByTestId('discover-install-confirm')).toHaveTextContent(
      'will run on this host'
    )
  }}
/>

<Story
  name="Add by URL: not an app"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('discover-add-url'), 'https://example.org/bad.json')
    await userEvent.click(canvas.getByTestId('discover-add-preview'))
    await expect(await canvas.findByTestId('discover-add-error')).toHaveTextContent('bad app ref')
  }}
/>
