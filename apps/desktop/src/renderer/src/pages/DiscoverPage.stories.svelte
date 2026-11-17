<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import DiscoverPage from './DiscoverPage.svelte'
  import { apps } from '../lib/apps'

  const app = apps.discover

  /** Discover: the landing window. */
  const { Story } = defineMeta({
    title: 'Apps/Discover',
    component: DiscoverPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
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
