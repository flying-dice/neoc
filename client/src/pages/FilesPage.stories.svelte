<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import FilesPage from './FilesPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import {
    createFakeHost,
    FAKE_AGENTS_APP,
    FAKE_CALCULATOR,
    FAKE_COMPUTER,
    FAKE_DOCUMENTS,
    FAKE_HOME
  } from '../shell/fakeHost'
  import { useUploader } from '../shell/upload'

  const app = apps.files
  const installed = [FAKE_CALCULATOR, FAKE_AGENTS_APP]

  /**
   * A host with a file system of its own and two apps, one of which keeps documents; `options` change what its
   * files answer. Uploads go to the fake, never to a host.
   */
  const useHost = (options: Parameters<typeof createFakeHost>[0] = {}): void => {
    const fake = createFakeHost({
      apps: installed,
      files: FAKE_DOCUMENTS,
      computer: FAKE_COMPUTER,
      home: FAKE_HOME,
      ...options
    })
    host.useFake(fake, options.apps ?? installed)
    useUploader(fake.upload)
  }

  /** The files a person picks to upload. */
  const picked = (...names: string[]): File[] =>
    names.map((name) => new File(['a file from this device'], name))

  /**
   * Files: the places (Home, This Computer, the apps), folders and files, the path field, search, a new folder,
   * uploads with their conflicts, removing, and the loading, empty, error and no-permission states.
   */
  const { Story } = defineMeta({
    title: 'Apps/Files',
    component: FilesPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    beforeEach: () => useHost(),
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <FilesPage />
  </AppWindow>
{/snippet}

<Story
  name="Places"
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('files-place-home')).toBeVisible()
    await expect(canvas.getByTestId('files-place-computer')).toBeVisible()
    // the app that keeps documents comes first
    await expect(await canvas.findByTestId('files-app-agents')).toHaveTextContent('5 items')
    await expect(canvas.getByTestId('files-app-calculator')).toHaveTextContent('No documents')
  }}
/>

<Story
  name="Apps only"
  beforeEach={() => useHost({ computer: undefined })}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('files-app-agents')).toBeVisible()
    await expect(canvas.queryByTestId('files-place-computer')).toBeNull()
  }}
/>

<Story
  name="Home"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await expect(await canvas.findByTestId('files-entry-todo.md')).toHaveTextContent('310 bytes')
    await expect(canvas.getByTestId('files-path')).toHaveValue('/Users/me')
    await expect(canvas.getByTestId('files-nav-back')).toHaveTextContent('Users')
  }}
/>

<Story
  name="Top of the disk"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-computer'))
    await expect(await canvas.findByTestId('files-entry-Users')).toBeVisible()
    await expect(canvas.getByTestId('files-path')).toHaveValue('/')
  }}
/>

<Story
  name="Go to a path"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-computer'))
    const path = await canvas.findByTestId('files-path')
    await userEvent.clear(path)
    await userEvent.type(path, '/Users/me/Documents{Enter}')
    await expect(await canvas.findByTestId('files-entry-letter.txt')).toBeVisible()
  }}
/>

<Story
  name="No permission"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-computer'))
    await userEvent.click(await canvas.findByTestId('files-entry-private'))
    await expect(await canvas.findByTestId('files-error')).toHaveTextContent(
      'You do not have permission to open this folder.'
    )
    await expect(canvas.getByTestId('files-nav-back')).toBeVisible()
  }}
/>

<Story
  name="Documents"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await expect(await canvas.findByTestId('files-entry-refactor-plan.md')).toHaveTextContent(
      '2.3 KB'
    )
    await expect(canvas.getByTestId('files-entry-transcripts')).toBeVisible()
    await expect(canvas.queryByTestId('files-path')).toBeNull()
  }}
/>

<Story
  name="Folder"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await userEvent.click(await canvas.findByTestId('files-entry-transcripts'))
    await expect(await canvas.findByTestId('files-entry-coder-2026-06-07.md')).toBeVisible()
    await expect(canvas.getByTestId('files-nav-back')).toHaveTextContent('Agents')
  }}
/>

<Story
  name="Search"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await canvas.findByTestId('files-entry-refactor-plan.md')
    await userEvent.type(canvas.getByTestId('files-search'), 'plan')
    await expect(canvas.getByTestId('files-entry-refactor-plan.md')).toBeVisible()
    await expect(canvas.queryByTestId('files-entry-uptime-report.csv')).toBeNull()
  }}
/>

<Story
  name="No results"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await canvas.findByTestId('files-entry-refactor-plan.md')
    await userEvent.type(canvas.getByTestId('files-search'), 'zzz')
    await expect(canvas.getByTestId('files-no-results')).toBeVisible()
  }}
/>

<Story
  name="Empty folder"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-calculator'))
    await expect(await canvas.findByTestId('files-empty')).toHaveTextContent(
      'Calculator has not saved any files.'
    )
  }}
/>

<Story
  name="New folder"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await userEvent.click(await canvas.findByTestId('files-new-folder'))
    // nothing typed: nothing to send, and nothing wrong to say
    await expect(canvas.getByTestId('files-folder-create')).toBeDisabled()
    await expect(canvas.queryByTestId('files-folder-problem')).toBeNull()
    await userEvent.type(canvas.getByTestId('files-folder-name'), 'Scans 2026')
    await expect(canvas.getByTestId('files-folder-create')).toBeEnabled()
  }}
/>

<Story
  name="New folder, name taken"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await userEvent.click(await canvas.findByTestId('files-new-folder'))
    await userEvent.type(canvas.getByTestId('files-folder-name'), 'documents')
    await expect(canvas.getByTestId('files-folder-problem')).toHaveTextContent('already here')
    await expect(canvas.getByTestId('files-folder-create')).toBeDisabled()
  }}
/>

<Story
  name="New folder, made"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await userEvent.click(await canvas.findByTestId('files-new-folder'))
    await userEvent.type(canvas.getByTestId('files-folder-name'), 'Scans{Enter}')
    await expect(await canvas.findByTestId('files-entry-Scans')).toBeVisible()
    await expect(canvas.queryByTestId('files-folder-name')).toBeNull()
  }}
/>

<Story
  name="Uploaded"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await canvas.findByTestId('files-entry-todo.md')
    await userEvent.upload(
      canvas.getByTestId('files-upload-input'),
      picked('holiday.jpg', 'notes.txt')
    )
    await expect(await canvas.findByTestId('files-upload-holiday.jpg')).toHaveTextContent(
      'Uploaded'
    )
    // the new files are in the list
    await expect(await canvas.findByTestId('files-entry-holiday.jpg')).toBeVisible()
    await expect(canvas.getByTestId('files-entry-notes.txt')).toBeVisible()
  }}
/>

<Story
  name="Uploading"
  beforeEach={() => useHost({ uploadStallsAt: 0.42 })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await canvas.findByTestId('files-entry-todo.md')
    await userEvent.upload(canvas.getByTestId('files-upload-input'), picked('film.mov', 'next.txt'))
    await expect(await canvas.findByTestId('files-upload-film.mov')).toHaveTextContent('42%')
    await expect(canvas.getByTestId('files-upload-next.txt')).toHaveTextContent('Waiting')
  }}
/>

<Story
  name="Upload failed"
  beforeEach={() => useHost({ uploadError: 'todo.txt: The disk is full.' })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await canvas.findByTestId('files-entry-todo.md')
    await userEvent.upload(canvas.getByTestId('files-upload-input'), picked('todo.txt'))
    await expect(await canvas.findByTestId('files-upload-todo.txt')).toHaveTextContent(
      'The disk is full.'
    )
    await expect(canvas.getByTestId('files-uploads-dismiss')).toBeVisible()
  }}
/>

<Story
  name="Upload, name taken"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await canvas.findByTestId('files-entry-todo.md')
    await userEvent.upload(canvas.getByTestId('files-upload-input'), picked('todo.md'))
    await expect(await canvas.findByTestId('files-conflict-alert')).toHaveTextContent(
      '“todo.md” is already here'
    )
  }}
/>

<Story
  name="Upload, kept both"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-place-home'))
    await canvas.findByTestId('files-entry-todo.md')
    await userEvent.upload(canvas.getByTestId('files-upload-input'), picked('todo.md'))
    await userEvent.click(await canvas.findByTestId('files-conflict-alert-action-keep-both'))
    await expect(await canvas.findByTestId('files-entry-todo (2).md')).toBeVisible()
    await expect(canvas.getByTestId('files-entry-todo.md')).toHaveTextContent('310 bytes')
  }}
/>

<Story
  name="Remove"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await userEvent.click(await canvas.findByTestId('files-entry-uptime-report.csv-remove'))
    await expect(await canvas.findByTestId('files-remove-alert')).toHaveTextContent(
      'Remove “uptime-report.csv”?'
    )
  }}
/>

<Story
  name="Remove a folder"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await userEvent.click(await canvas.findByTestId('files-entry-transcripts-remove'))
    // the question says how much goes with it
    await expect(await canvas.findByTestId('files-remove-alert')).toHaveTextContent(
      'the 2 items in it'
    )
  }}
/>

<Story
  name="Removed"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('files-app-agents'))
    await userEvent.click(await canvas.findByTestId('files-entry-uptime-report.csv-remove'))
    await userEvent.click(await canvas.findByTestId('files-remove-alert-action-remove'))
    await waitFor(() => expect(canvas.queryByTestId('files-entry-uptime-report.csv')).toBeNull())
    await expect(canvas.getByTestId('files-entry-refactor-plan.md')).toBeVisible()
  }}
/>

<Story
  name="Loading"
  beforeEach={() => useHost({ filesPending: true, computer: undefined })}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('files-loading')).toBeVisible()
  }}
/>

<Story
  name="Error"
  beforeEach={() => useHost({ filesError: 'The files of this host could not be read.' })}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('files-error')).toHaveTextContent('could not be read')
    await expect(canvas.getByTestId('files-retry')).toBeVisible()
  }}
/>

<Story
  name="No apps"
  beforeEach={() => useHost({ apps: [], computer: undefined })}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('files-apps-empty')).toBeVisible()
  }}
/>
