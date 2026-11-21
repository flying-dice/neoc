<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import FilesPage from './FilesPage.svelte'
  import { apps } from '../lib/apps.svelte'

  const app = apps.files

  /** Files: locations and recent files. */
  const { Story } = defineMeta({
    title: 'Apps/Files',
    component: FilesPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <FilesPage />
  </AppWindow>
{/snippet}

<Story name="Browse" />

<Story
  name="Search"
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('files-search'), 'plan')
    await expect(canvas.getByTestId('files-recent-refactor-plan.md')).toBeVisible()
    await expect(canvas.queryByTestId('files-recent-inbox-digest.txt')).toBeNull()
  }}
/>
