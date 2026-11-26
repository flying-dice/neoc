<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import PageList from './PageList.svelte'
  type Page = VosApp.Projects.Page
  import { tree, type Leaf } from '../lib/catalog'
  import { PAGES } from '../lib/samples'

  interface Args {
    pages: readonly Leaf[]
    draft: string
    onopen: (page: Page) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  const pages = tree(PAGES.filter((p) => p.project === 1))
  const onopen = fn<(page: Page) => void>()
  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** A project's pages, each followed by the pages under it. */
  const { Story } = defineMeta({
    title: 'Organisms/PageList',
    component: PageList,
    tags: ['autodocs'],
    args: { pages, draft: '', onopen, ondraft, onadd },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:460px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <PageList {...args} />
  </div>
{/snippet}

<Story
  name="With pages"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-page-row-2')).toHaveProperty('subheading', 'In Design')
    await userEvent.click(canvas.getByTestId('projects-page-row-2'))
    await expect(onopen).toHaveBeenCalledWith(PAGES[1])
  }}
/>

<Story
  name="Empty"
  args={{ pages: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-pages-empty')).toBeVisible()
  }}
/>
