<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { ComponentProps } from 'svelte'
  import GraphPane from './GraphPane.svelte'

  type Args = ComponentProps<typeof GraphPane>

  const onopen = fn<(id: number) => void>()
  const onback = fn<() => void>()

  const NODES = ['Garden plan', 'Tomatoes', 'Herbs', 'Reading list', 'Book notes', 'A title long enough that it has to be cut short', '2026-06-09'].map(
    (title, i) => ({ id: i + 1, title })
  )
  const LINKS = [
    { from: 1, to: 2 },
    { from: 1, to: 3 },
    { from: 2, to: 3 },
    { from: 3, to: 1 },
    { from: 4, to: 5 },
    { from: 7, to: 1 }
  ]

  /** The graph of a notebook. It keeps nothing but its own size: the note whose dot is pressed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/GraphPane',
    component: GraphPane,
    tags: ['autodocs'],
    args: { title: 'Notebook', nodes: NODES, links: LINKS, onopen, onback },
    beforeEach: () => {
      onopen.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <GraphPane {...args} />
  </div>
{/snippet}

<!-- Notes that link to each other sit together; the open note's dot and its links are marked. -->
<Story
  name="A notebook's graph"
  args={{ selected: 1 }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-graph-count')).toHaveTextContent('7 notes, 6 links')
    await expect(canvas.getByRole('button', { name: 'Herbs' })).toBeVisible()
  }}
/>

<Story name="Notes without links" args={{ links: [] }} />

<Story
  name="No notes"
  args={{ nodes: [], links: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-graph-empty')).toBeVisible()
  }}
/>

<Story name="In a narrow window" args={{ back: true }} />

<Story
  name="Opening a note"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('notebooks-node-3'))
    await expect(onopen).toHaveBeenCalledWith(3)
    canvas.getByTestId('notebooks-node-2').focus()
    await userEvent.keyboard('{Enter}')
    await expect(onopen).toHaveBeenCalledWith(2)
  }}
/>
