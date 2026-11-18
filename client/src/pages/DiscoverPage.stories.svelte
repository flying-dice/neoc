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

<!-- The agents of this host come from its roster (agents.list): nothing to install, so no GET. -->
<Story
  name="Agents on this host"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('discover-section-agents'))
    const agent = await canvas.findByTestId('discover-agent-scripted')
    await expect(agent).toHaveTextContent('Scripted agent')
    await expect(agent).toHaveTextContent('Answers by script, for tests')
    await expect(canvas.queryByTestId('discover-agent-scripted-get')).toBeNull()
    await expect(canvas.queryByTestId('discover-listing-coder')).toBeNull() // the old sample listings are gone
  }}
/>

<Story
  name="No search results"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('discover-search'), 'spreadsheet')
    await expect(await canvas.findByText('No apps match “spreadsheet”.')).toBeVisible()
    await expect(canvas.getByText('No agents match “spreadsheet”.')).toBeVisible()
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
    // every button of the flow is a target a finger can hit (44 pt)
    const tall = (id: string): number => canvas.getByTestId(id).getBoundingClientRect().height
    await expect(tall('discover-add-preview')).toBeGreaterThanOrEqual(44)
    await userEvent.click(canvas.getByTestId('discover-add-preview'))
    await expect(await canvas.findByTestId('discover-add-preview-card')).toHaveTextContent(
      'Version 0.2.0'
    )
    await expect(tall('discover-add-install')).toBeGreaterThanOrEqual(44)
    await expect(tall('discover-add-cancel')).toBeGreaterThanOrEqual(44)
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
    await expect(canvas.queryByTestId('discover-agent-scripted-get')).toBeNull() // listed, with nothing to open
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

<!-- With the Agents app running, each agent offers OPEN (the Agents app); the sidebar narrows the page to the agents. -->
<Story
  name="Agents on this host, Agents app installed"
  beforeEach={() => {
    withHost([FAKE_CALCULATOR, FAKE_AGENTS_APP])()
    return closeWindows
  }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('discover-section-apps'))
    await expect(await canvas.findByTestId('discover-apps')).toBeVisible()
    await expect(canvas.queryByTestId('discover-agents')).toBeNull()
    await userEvent.click(canvas.getByTestId('discover-section-agents'))
    await expect(canvas.queryByTestId('discover-apps')).toBeNull()
    await userEvent.click(await canvas.findByTestId('discover-agent-scripted-get'))
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
