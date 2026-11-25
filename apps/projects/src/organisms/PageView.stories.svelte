<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { textArea } from '@vos/storybook/kit'
  import PageView from './PageView.svelte'
  import type { Page } from '@projects/rpc'
  import { tree, type Leaf } from '../lib/catalog'
  import { BODIES, PAGES } from '../lib/samples'
  import type { PageDraft } from '../lib/projects.svelte'

  interface Args {
    leaf: Leaf
    body: string | null
    edit: PageDraft | null
    subpages: readonly Page[]
    draft: string
    onrewrite: () => void
    ontype: (change: Partial<PageDraft>) => void
    onsave: () => void
    ondiscard: () => void
    onremove: (page: Page) => void
    onopen: (page: Page) => void
    ondraft: (text: string) => void
    onadd: () => void
    onback: () => void
  }

  const leaves = tree(PAGES.filter((p) => p.project === 1))
  const design = leaves[0]!
  const colours = leaves[1]!
  const onrewrite = fn<() => void>()
  const ontype = fn<(change: Partial<PageDraft>) => void>()
  const onsave = fn<() => void>()
  const ondiscard = fn<() => void>()
  const onremove = fn<(page: Page) => void>()
  const onopen = fn<(page: Page) => void>()
  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()
  const onback = fn<() => void>()

  /** One page, open: read, or rewritten. It keeps nothing: what is typed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/PageView',
    component: PageView,
    tags: ['autodocs'],
    args: { leaf: design, body: BODIES[1], edit: null, subpages: [PAGES[1]!, PAGES[2]!], draft: '', onrewrite, ontype, onsave, ondiscard, onremove, onopen, ondraft, onadd, onback },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:720px;height:600px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <PageView {...args} />
  </div>
{/snippet}

<Story
  name="Reading"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-page-title')).toHaveTextContent('Design')
    await expect(canvas.getByTestId('projects-page-body')).toHaveTextContent('Calm, plain, and quick to load.')
    await userEvent.click(canvas.getByTestId('projects-page-edit'))
    await expect(onrewrite).toHaveBeenCalled()
  }}
/>

<!-- A page under another says where it is, and the way back is to that page. -->
<Story
  name="A page under another"
  args={{ leaf: colours, body: BODIES[2], subpages: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-page-path')).toHaveTextContent('Design')
    await expect(canvas.getByTestId('projects-page-back')).toHaveProperty('label', 'Back to Design')
  }}
/>

<Story name="Not read yet" args={{ body: null, subpages: [] }} />

<Story name="Nothing written" args={{ body: '', subpages: [] }} />

<Story
  name="Rewriting"
  args={{ edit: { title: 'Design', body: BODIES[1] } }}
  play={async ({ canvas, userEvent }) => {
    await expect(textArea(canvas.getByTestId('projects-page-text'))).toHaveValue(BODIES[1])
    await userEvent.click(canvas.getByTestId('projects-page-done'))
    await expect(onsave).toHaveBeenCalled()
  }}
/>

<!-- A page needs a title: Done cannot be pressed without one. -->
<Story
  name="Rewriting, with no title"
  args={{ edit: { title: '', body: BODIES[1] } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-page-done')).toHaveProperty('disabled', true)
  }}
/>
