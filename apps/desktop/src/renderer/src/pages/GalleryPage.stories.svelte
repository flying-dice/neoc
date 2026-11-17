<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import GalleryPage from './GalleryPage.svelte'
  import { apps } from '../lib/apps'

  const app = apps.gallery

  /** UI Kit: the living catalogue of @agentos/ui. */
  const { Story } = defineMeta({
    title: 'Apps/UI Kit',
    component: GalleryPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <GalleryPage />
  </AppWindow>
{/snippet}

<Story name="Catalogue" />

<Story
  name="Alert"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('gallery-show-alert'))
    await expect(canvas.getByTestId('gallery-alert')).toBeVisible()
  }}
/>

<Story
  name="Sheet"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('gallery-present-sheet'))
    await expect(canvas.getByTestId('gallery-sheet')).toBeVisible()
  }}
/>
