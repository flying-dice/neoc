<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import Toggle from '../atoms/Toggle.svelte'
  import ListRow from './ListRow.svelte'
  import SwipeAction from './SwipeAction.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/ListRow',
    component: ListRow,
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })

  const onclick = fn()
  const frame = 'width:360px;background:var(--bg-elevated);border-radius:10px;overflow:hidden'
</script>

<!-- The plain cell the shell uses: a button when it has onclick. -->
<Story name="Plain cell" asChild parameters={{ controls: { disable: true } }}>
  <div style={frame}>
    <ListRow testid="plain-a" title="Wi-Fi" value="Home" icon="wifi" {onclick} />
    <ListRow testid="plain-b" title="About" subtitle="Version 1.0" chevron {onclick} />
    <ListRow testid="plain-c" title="Reset…" destructive {onclick} />
    <ListRow testid="plain-d" title="Add Account…" action {onclick} />
  </div>
</Story>

{#snippet trailing()}<Toggle testid="managed-toggle" label="Notify me" />{/snippet}
<!-- A managed row: a listitem with an invisible full-row button, so trailing controls are siblings, never nested. -->
<Story
  name="Managed row, tappable with a trailing control"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    onclick.mockClear()
    const row = canvas.getByRole('listitem')
    await expect(row).toHaveAttribute('aria-posinset', '2')
    await expect(row).toHaveAttribute('aria-setsize', '9')
    await userEvent.click(canvas.getByTestId('managed-tap'))
    await expect(onclick).toHaveBeenCalledTimes(1)
    // The trailing switch is its own control: toggling it does not tap the row.
    await userEvent.click(canvas.getByRole('switch', { name: 'Notify me' }))
    await expect(onclick).toHaveBeenCalledTimes(1)
    await expect(canvas.getByRole('button', { name: 'Reminders, Daily at 9' })).toBeVisible()
  }}
>
  <div style={frame} role="list">
    <ListRow testid="managed" title="Reminders" subtitle="Daily at 9" role="listitem" rowKey="r" posinset={2} setsize={9} {onclick} {trailing} />
  </div>
</Story>

<!-- A selectable row is an option; aria-selected, Enter and Space select it. -->
<Story
  name="Option, selected and not"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    onclick.mockClear()
    const [first, second] = canvas.getAllByRole('option')
    await expect(first).toHaveAttribute('aria-selected', 'true')
    await expect(second).toHaveAttribute('aria-selected', 'false')
    second.focus()
    await userEvent.keyboard('{Enter}')
    await userEvent.keyboard(' ')
    await expect(onclick).toHaveBeenCalledTimes(2)
  }}
>
  <div style={frame} role="listbox" aria-label="Pick one">
    <ListRow testid="opt-a" title="Light" role="option" selected {onclick} />
    <ListRow testid="opt-b" title="Dark" role="option" {onclick} />
  </div>
</Story>

<Story name="Multiple selection, check circles" asChild parameters={{ controls: { disable: true } }}>
  <div style={frame} role="listbox" aria-multiselectable="true" aria-label="Pick several">
    <ListRow testid="multi-a" title="Milk" role="option" selected check {onclick} />
    <ListRow testid="multi-b" title="Eggs" role="option" check={false} {onclick} />
    <ListRow testid="multi-c" title="Bread" role="option" selected check {onclick} />
  </div>
</Story>

{#snippet swipeTrailing()}
  <SwipeAction testid="swipe-pin" title="Pin" icon="star" />
  <SwipeAction testid="swipe-delete" title="Delete" icon="trash" role="destructive" />
{/snippet}
{#snippet swipeLeading()}<SwipeAction testid="swipe-read" title="Read" role="neutral" />{/snippet}
<!-- Swipe actions: drag the face, or Alt+Left from the keyboard (see the List stories); both groups stay in the a11y tree. -->
<Story
  name="Swipe actions, open"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getAllByTestId('swipe-delete')[0]).toBeVisible())
    await expect(canvas.getAllByRole('button', { name: /Delete/ }).length).toBeGreaterThan(0)
  }}
>
  <div style={frame} role="list">
    <ListRow testid="swipe-open" title="Groceries" subtitle="Note 1" role="listitem" open="trailing" {swipeTrailing} {swipeLeading} />
    <ListRow testid="swipe-closed" title="Ideas" subtitle="Note 2" role="listitem" {swipeTrailing} {swipeLeading} />
  </div>
</Story>

<Story name="Reorder handle and placeholder" asChild parameters={{ controls: { disable: true } }}>
  <div style={frame} role="list">
    <ListRow testid="handle-a" title="Milk" role="listitem" reorder />
    <ListRow testid="handle-b" title="Eggs" role="listitem" reorder />
    <ListRow testid="loading-a" title="" role="listitem" placeholder />
    <ListRow testid="loading-b" title="" role="listitem" placeholder />
  </div>
</Story>

<Story name="Narrow, long text truncates" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:280px;background:var(--bg-elevated);border-radius:10px;overflow:hidden">
    <ListRow testid="long" title="A very long title that cannot fit in a narrow window at all" subtitle="And a long subtitle that is cut with an ellipsis too" role="listitem" chevron {onclick} />
  </div>
</Story>
