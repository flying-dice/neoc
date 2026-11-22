<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Site } from 'app:rpc'
  import SiteRow from './SiteRow.svelte'
  import { NO_FOLDER, SETTINGS, site } from '../lib/fakeApi'
  import { addressesOf, type Address } from '../lib/format'

  interface Args {
    site: Site
    addresses: readonly Address[]
    busy: boolean
    disabled: boolean
    onenable: (enabled: boolean) => void
    onopen: (url: string) => void
    onedit: () => void
    onremove: () => void
  }

  const onenable = fn<(enabled: boolean) => void>()
  const onopen = fn<(url: string) => void>()
  const onedit = fn<() => void>()
  const onremove = fn<() => void>()
  const blog = site(1, 'Blog', '/Users/me/Sites/blog', 8081, ['blog.home'])
  const reached = (one: Site): Address[] => addressesOf(one, SETTINGS, 'nas.local')

  /** One site: where its files are, and every address it is reached at. */
  const { Story } = defineMeta({
    title: 'Molecules/SiteRow',
    component: SiteRow,
    tags: ['autodocs'],
    args: { site: blog, addresses: reached(blog), busy: false, disabled: false, onenable, onopen, onedit, onremove },
    beforeEach: () => {
      onenable.mockClear()
      onopen.mockClear()
      onedit.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:480px;margin:0;padding:0;background:var(--bg-secondary);color:var(--label);font:var(--type-body)">
    <SiteRow {...args} />
  </ul>
{/snippet}

<Story
  name="A port and a name"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('pages-site-1-name')).toHaveTextContent('Blog')
    await expect(canvas.getByTestId('pages-site-1-open-0')).toHaveTextContent('nas.local:8081')
    await userEvent.click(canvas.getByTestId('pages-site-1-open-1'))
    await expect(onopen).toHaveBeenCalledWith('http://blog.home/')
    await userEvent.click(canvas.getByTestId('pages-site-1-edit'))
    await expect(onedit).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('pages-site-1-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A site that is off is not served: its addresses are words, not buttons. -->
<Story
  name="Off"
  args={{
    site: { ...blog, enabled: false },
    addresses: reached(blog)
  }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('pages-site-1-open-0')).toBeNull()
    await expect(canvas.getByTestId('pages-site-1-address-0')).toHaveTextContent('nas.local:8081')
  }}
/>

<!-- On, and left out of the server: the folder is not there. -->
<Story
  name="The folder is not there"
  args={{ site: { ...blog, problem: NO_FOLDER } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-site-1-problem')).toHaveTextContent('Not served: The folder is not there.')
  }}
/>

<Story name="Being changed" args={{ busy: true }} />

<!-- A long name, a long path and many names are cut or wrapped, and the buttons stay. -->
<Story
  name="Long"
  args={{
    site: site(9, 'The Collected Notes of a Household That Writes Everything Down', `/Users/me/${'a-long-folder-name/'.repeat(6)}site`, 8099, [
      'notes.home',
      'www.notes.home',
      'everything-we-ever-wrote-down.notes.home'
    ]),
    addresses: reached(
      site(9, '', '', 8099, ['notes.home', 'www.notes.home', 'everything-we-ever-wrote-down.notes.home'])
    )
  }}
/>
