<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Settings, Site } from '@pages/rpc'
  import SiteList from './SiteList.svelte'
  import { NO_FOLDER, SETTINGS, SITES } from '../lib/fakeApi'
  import type { Editing, SiteDraft } from '../lib/pages.svelte'

  interface Args {
    sites: readonly Site[]
    settings: Settings
    machine: string
    editing: Editing | null
    working: boolean
    busy: (site: Site) => boolean
    onadd: () => void
    onedit: (site: Site) => void
    ondraft: (change: Partial<SiteDraft>) => void
    onsave: () => void
    oncancel: () => void
    onenable: (site: Site, enabled: boolean) => void
    onremove: (site: Site) => void
    onopen: (url: string) => void
  }

  const onadd = fn<() => void>()
  const onedit = fn<(site: Site) => void>()
  const onremove = fn<(site: Site) => void>()

  /** The sites that are hosted, or the form for the one that is typed. */
  const { Story } = defineMeta({
    title: 'Organisms/SiteList',
    component: SiteList,
    tags: ['autodocs'],
    args: {
      sites: SITES,
      settings: SETTINGS,
      machine: 'nas.local',
      editing: null,
      working: false,
      busy: () => false,
      onadd,
      onedit,
      ondraft: fn(),
      onsave: fn(),
      oncancel: fn(),
      onenable: fn(),
      onremove,
      onopen: fn()
    },
    argTypes: { busy: { control: false } },
    beforeEach: () => {
      onadd.mockClear()
      onedit.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:480px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <SiteList {...args} />
  </div>
{/snippet}

<Story
  name="A home server"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('pages-sites-summary')).toHaveTextContent('3 sites')
    await userEvent.click(canvas.getByTestId('pages-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('pages-site-2-edit'))
    await expect(onedit).toHaveBeenCalledWith(SITES[1])
    await userEvent.click(canvas.getByTestId('pages-site-3-remove'))
    await expect(onremove).toHaveBeenCalledWith(SITES[2])
  }}
/>

<Story
  name="No sites"
  args={{ sites: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-empty')).toBeVisible()
    await expect(canvas.getByTestId('pages-sites-summary')).toHaveTextContent('No sites')
  }}
/>

<Story name="One being changed" args={{ working: true, busy: (site: Site) => site.id === 2 }} />

<Story name="One without its folder" args={{ sites: SITES.map((site) => (site.id === 1 ? { ...site, problem: NO_FOLDER } : site)) }} />

<!-- While a site is typed the form stands where the list was. -->
<Story
  name="A new site being typed"
  args={{ editing: { site: null, draft: { name: 'Photos', folder: '/Users/me/Sites/photos', port: '8082', hostnames: '' } } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-site-save')).toHaveTextContent('Add Site')
    await expect(canvas.queryByTestId('pages-sites')).toBeNull()
  }}
/>
