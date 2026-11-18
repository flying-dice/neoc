<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import DiscoverPage from './DiscoverPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import { windowManager } from '../lib/windowManager.svelte'
  import { createFakeHost, FAKE_AGENTS_APP, FAKE_CALCULATOR } from '../shell/fakeHost'
  import type { AppSummary, HostApi } from '../shell/hostApi'

  const app = apps.discover

  /** The host a story talks to: the Calculator installed, Notes offered by the registry. */
  const withHost =
    (installed: AppSummary[] = [FAKE_CALCULATOR], registryErrors: string[] = []) =>
    () =>
      host.useFake(createFakeHost({ apps: installed, registryErrors }), installed)

  /** A host from another version: its answers to the shell's own calls are not the shapes this client knows. */
  const withUnknownShapes = async (): Promise<void> => {
    const base = createFakeHost()
    const api: HostApi = {
      call: <T,>(method: string, params: Record<string, unknown> = {}): Promise<T> =>
        method === 'app.list'
          ? Promise.resolve({ apps: null } as T)
          : method === 'app.available'
            ? Promise.resolve({ apps: 5, errors: 'none' } as T)
            : base.call<T>(method, params),
      onNotification: (listener) => base.onNotification(listener)
    }
    host.useFake(api, [FAKE_CALCULATOR])
    await host.refresh()
  }

  const closeWindows = (): void => {
    for (const win of [...windowManager.windows]) windowManager.close(win.id)
  }

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

<!-- A host without the Agents app: nothing offers to open it (no OPEN, no "Open Agents"), and nothing throws. -->
<Story
  name="Agents app not installed"
  play={async ({ canvas }) => {
    await canvas.findByTestId('discover-app-calculator')
    await expect(canvas.queryByTestId('discover-open')).toBeNull()
    await expect(canvas.queryByTestId('discover-hero-open-agents')).toBeNull()
    await expect(canvas.getByTestId('discover-hero-ui-kit')).toBeVisible()
  }}
/>

<!-- With the Agents app running, both entry points show and OPEN brings its window forward. -->
<Story
  name="Agents app installed"
  beforeEach={() => {
    withHost([FAKE_CALCULATOR, FAKE_AGENTS_APP])()
    return closeWindows
  }}
  play={async ({ canvas, userEvent }) => {
    await expect(await canvas.findByTestId('discover-open')).toBeVisible()
    await expect(canvas.getByTestId('discover-hero-open-agents')).toBeVisible()
    await userEvent.click(canvas.getByTestId('discover-open'))
    await expect(windowManager.isOpen('agents')).toBe(true)
  }}
/>

<!-- The numbers are the host's: agents on the roster, apps running. No made-up badge on Updates. -->
<Story
  name="Counts come from the host"
  beforeEach={withHost([FAKE_CALCULATOR, notesDisabled])}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('discover-account')).toHaveTextContent('1 agent')
    await expect(canvas.getByTestId('discover-account')).not.toHaveTextContent('4 agents')
    await expect(await canvas.findByText('1 of 2')).toBeVisible()
    await expect(canvas.getByTestId('discover-section-updates')).not.toHaveTextContent(/\d/)
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
    await expect(canvas.getByTestId('discover-add-url')).toBeVisible()
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
