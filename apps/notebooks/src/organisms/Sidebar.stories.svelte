<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog, pickerSelect } from '@vos/storybook/kit'
  import type { ComponentProps } from 'svelte'
  import type { Summary } from 'app:rpc'
  import Sidebar from './Sidebar.svelte'
  import { NOTEBOOK } from '../lib/fakeApi'

  type Args = ComponentProps<typeof Sidebar>

  const NOW = new Date(2026, 5, 9, 9, 41)
  const at = (day: number, hour: number): string => new Date(2026, 5, day, hour, 5).toISOString()
  const summary = (id: number, title: string, given: Partial<Summary> = {}): Summary => ({
    id,
    notebook: 1,
    title,
    excerpt: '',
    tags: [],
    pinned: false,
    updated: at(9, 8),
    ...given
  })
  const NOTES = [
    summary(1, 'Garden plan', { excerpt: 'The beds for this year, with what goes in each.', tags: ['garden', 'plans'], pinned: true }),
    summary(2, 'Tomatoes', { excerpt: 'Sow indoors in March, plant out in May.', tags: ['garden'], updated: at(8, 17) }),
    summary(3, 'A title long enough that it has to be cut short in the sidebar', { excerpt: 'And a first line that is just as long, so it is cut too.', updated: at(1, 12) })
  ]
  const TAGS = [
    { tag: 'garden', count: 2 },
    { tag: 'plans', count: 1 }
  ]

  const mocks = {
    onnotebook: fn<(id: number) => void>(),
    onnew: fn<() => void>(),
    ondaily: fn<() => void>(),
    onswitch: fn<() => void>(),
    ongraph: fn<() => void>(),
    onsearch: fn<(query: string) => void>(),
    ontag: fn<(tag: string) => void>(),
    onopen: fn<(id: number) => void>(),
    ondraft: fn<(name: string) => void>(),
    onaddnotebook: fn<() => Promise<boolean>>(async () => true),
    onremovenotebook: fn<() => void>()
  }

  /** The kit's text field holds the real input in its shadow tree. */
  const input = (canvas: { getByTestId(id: string): HTMLElement }, id: string): HTMLInputElement =>
    canvas.getByTestId(id).shadowRoot!.querySelector('input')!

  /** A notebook's contents. It keeps only whether a notebook is being named or deleted: everything pressed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/Sidebar',
    component: Sidebar,
    tags: ['autodocs'],
    args: { notebooks: [NOTEBOOK], notebook: NOTEBOOK, notes: [], tags: [], query: '', now: NOW, draft: '', ...mocks },
    argTypes: { now: { control: false } },
    beforeEach: () => {
      for (const mock of Object.values(mocks)) mock.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:320px;height:560px;color:var(--label);font:var(--type-body)">
    <Sidebar {...args} />
  </div>
{/snippet}

<!-- A new library: one notebook, which cannot be deleted, and no notes in it. -->
<Story
  name="A new notebook"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-empty')).toBeVisible()
    await expect(canvas.queryByTestId('notebooks-delete-notebook')).toBeNull()
    await expect(canvas.queryByTestId('notebooks-tags')).toBeNull()
  }}
/>

<!-- Notes from before: the bookmarked one first, each with its first line and when it changed, and the tags they have. -->
<Story
  name="With notes"
  args={{ notebooks: [NOTEBOOK, { id: 2, name: 'Work' }], notes: NOTES, tags: TAGS, selected: 2 }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-note-1')).toHaveProperty('icon', 'star')
    await expect(canvas.getByTestId('notebooks-note-1')).toHaveProperty('value', '08:05')
    await expect(canvas.getByTestId('notebooks-note-2')).toHaveProperty('value', 'Yesterday')
    await expect(canvas.getByTestId('notebooks-note-2')).toHaveProperty('selected', true)
    await expect(canvas.getByTestId('notebooks-tag-garden')).toHaveTextContent('#garden 2')
    await expect(canvas.getByTestId('notebooks-delete-notebook')).toBeVisible()
  }}
/>

<Story
  name="Narrowed to a tag"
  args={{ notes: NOTES.slice(0, 2), tags: TAGS, tag: 'garden' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-tag-garden')).toHaveAttribute('aria-pressed', 'true')
  }}
/>

<Story
  name="What a search found"
  args={{ notes: NOTES, tags: TAGS, query: 'march', hits: [{ id: 2, title: 'Tomatoes', excerpt: 'Sow indoors in March, plant out in May.' }] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-hit-2')).toHaveProperty('subheading', 'Sow indoors in March, plant out in May.')
    await expect(canvas.queryByTestId('notebooks-notes')).toBeNull()
  }}
/>

<Story name="A search that found nothing" args={{ notes: NOTES, tags: TAGS, query: 'zebra', hits: [] }} />

<!-- The graph is what is beside the list: its button is pressed. -->
<Story name="With the graph shown" args={{ notes: NOTES, tags: TAGS, graphing: true }} />

<Story
  name="Pressing"
  tags={['play-only']}
  args={{ notebooks: [NOTEBOOK, { id: 2, name: 'Work' }], notes: NOTES, tags: TAGS }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notebooks-new'))
    await expect(mocks.onnew).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notebooks-daily'))
    await expect(mocks.ondaily).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notebooks-switch'))
    await expect(mocks.onswitch).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notebooks-graph-toggle'))
    await expect(mocks.ongraph).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notebooks-tag-plans'))
    await expect(mocks.ontag).toHaveBeenCalledWith('plans')
    await userEvent.type(input(canvas, 'notebooks-search'), 'm')
    await expect(mocks.onsearch).toHaveBeenCalledWith('m')
    await userEvent.selectOptions(pickerSelect(canvas.getByTestId('notebooks-notebook')), 'Work')
    await expect(mocks.onnotebook).toHaveBeenCalledWith(2)
  }}
/>

<!-- A notebook is named in a field that is there once it is asked for; a name that cannot be had is said under it. -->
<Story
  name="Naming a notebook"
  args={{ draft: 'Notebook', problem: 'There is already a notebook called Notebook.' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notebooks-add-notebook'))
    await expect(canvas.getByTestId('notebooks-new-notebook-problem')).toHaveTextContent('There is already a notebook called Notebook.')
  }}
/>

<!-- Deleting a notebook asks first. -->
<Story
  name="Deleting a notebook"
  tags={['play-only']}
  args={{ notebooks: [NOTEBOOK, { id: 2, name: 'Work' }], notes: NOTES }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notebooks-delete-notebook'))
    await expect(alertDialog(canvas.getByTestId('notebooks-confirm-notebook'))).toHaveTextContent('Its notes are deleted with it.')
    await userEvent.click(alertButton(canvas.getByTestId('notebooks-confirm-notebook'), 'action'))
    await expect(mocks.onremovenotebook).toHaveBeenCalledTimes(1)
  }}
/>
