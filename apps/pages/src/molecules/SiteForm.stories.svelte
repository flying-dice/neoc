<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SiteForm from './SiteForm.svelte'
  import type { SiteDraft } from '../lib/pages.svelte'

  interface Args {
    draft: SiteDraft
    adding: boolean
    sharedPort: number
    busy: boolean
    ondraft: (change: Partial<SiteDraft>) => void
    onsave: () => void
    oncancel: () => void
  }

  const ondraft = fn<(change: Partial<SiteDraft>) => void>()
  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()
  const typed: SiteDraft = { name: 'Blog', folder: '/Users/me/Sites/blog', port: '8081', hostnames: 'blog.home' }

  /** A site as it is typed. Nothing is saved until it has a name, a folder, and a port or a hostname. */
  const { Story } = defineMeta({
    title: 'Molecules/SiteForm',
    component: SiteForm,
    tags: ['autodocs'],
    args: { draft: { name: '', folder: '', port: '', hostnames: '' }, adding: true, sharedPort: 80, busy: false, ondraft, onsave, oncancel },
    beforeEach: () => {
      ondraft.mockClear()
      onsave.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <SiteForm {...args} />
  </div>
{/snippet}

<Story
  name="A new site, empty"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('pages-site-save'))
    await expect(onsave).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('pages-site-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="A new site, typed"
  args={{ draft: typed }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('pages-site-save')).toHaveTextContent('Add Site')
    await userEvent.click(canvas.getByTestId('pages-site-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A name and a folder are not enough: there must be a way to reach the site. -->
<Story
  name="No port and no hostname"
  args={{ draft: { ...typed, port: '', hostnames: '' } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('pages-site-save'))
    await expect(onsave).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Changing a site"
  args={{ draft: typed, adding: false, sharedPort: 8080 }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-site-save')).toHaveTextContent('Save')
  }}
/>

<Story name="While a change is in flight" args={{ draft: typed, busy: true }} />
