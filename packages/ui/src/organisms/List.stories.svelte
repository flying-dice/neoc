<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import ListRow from '../molecules/ListRow.svelte'
  import SwipeAction from '../molecules/SwipeAction.svelte'
  import List from './List.svelte'
  import ListSection from './ListSection.svelte'
  import { moveBefore, moveBy } from '../logic/reorder'
  import type { ListStyle } from '../logic/listContext'

  const { Story } = defineMeta({
    title: 'Organisms/List',
    component: List,
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })
</script>

<script lang="ts">
  let picked = $state('wifi')
  let multi = $state<string[]>(['milk'])
  let open = $state(true)
  let order = $state(['Milk', 'Eggs', 'Bread', 'Cheese'])
  let said = $state('')
  let rows = $state(['Groceries', 'Ideas', 'Trip plan'])

  function reorder(key: string, delta: -1 | 1): void {
    const m = moveBy(order, key, delta)
    if (m) {
      order = m.keys
      said = `${key} moved to position ${m.to + 1} of ${order.length}`
    }
  }
  function drop(key: string, before: string | null): void {
    const m = moveBefore(order, key, before)
    if (m) order = m.keys
  }
</script>

{#snippet styled(style: ListStyle)}
  <div style="width:360px;background:{style === 'plain' || style === 'inset' ? 'var(--bg-elevated)' : 'var(--bg-grouped-elevated)'}">
    <List testid="list-{style}" {style} selection="single" label="Settings ({style})">
      <ListSection header="Network">
        <ListRow testid="{style}-wifi" title="Wi-Fi" value="Home" icon="wifi" role="option" selected={picked === 'wifi'} rowKey="wifi" onclick={() => (picked = 'wifi')} />
        <ListRow testid="{style}-vpn" title="VPN" chevron role="option" selected={picked === 'vpn'} rowKey="vpn" onclick={() => (picked = 'vpn')} />
      </ListSection>
      <ListSection header="General" footer="Applies to this device.">
        <ListRow testid="{style}-about" title="About" subtitle="Version 1.0" chevron role="option" selected={picked === 'about'} rowKey="about" onclick={() => (picked = 'about')} />
        <ListRow testid="{style}-reset" title="Reset…" destructive role="option" selected={picked === 'reset'} rowKey="reset" onclick={() => (picked = 'reset')} />
      </ListSection>
    </List>
  </div>
{/snippet}

<Story name="Plain" asChild parameters={{ controls: { disable: true } }}>{@render styled('plain')}</Story>
<Story name="Inset" asChild parameters={{ controls: { disable: true } }}>{@render styled('inset')}</Story>
<Story
  name="Inset grouped, one tab stop and arrow keys"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByRole('listbox')).toBeVisible()
    const options = canvas.getAllByRole('option')
    // Exactly one option is in the tab order: the selected one.
    await waitFor(() => expect(options.filter((o) => o.tabIndex === 0)).toHaveLength(1))
    options[0].focus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(options[1]).toHaveFocus()
    await userEvent.keyboard('{End}')
    await expect(options[3]).toHaveFocus()
    await userEvent.keyboard('{Home}')
    await expect(options[0]).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await expect(options[0]).toHaveAttribute('aria-selected', 'true')
    await userEvent.keyboard('a') // type to select: "About"
    await expect(options[2]).toHaveFocus()
  }}
>
  {@render styled('insetGrouped')}
</Story>
<Story name="Grouped" asChild parameters={{ controls: { disable: true } }}>{@render styled('grouped')}</Story>
<Story name="Sidebar" asChild parameters={{ controls: { disable: true } }}>{@render styled('sidebar')}</Story>

<Story
  name="Multiple selection"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByRole('listbox')).toHaveAttribute('aria-multiselectable', 'true')
    const eggs = canvas.getByRole('option', { name: 'Eggs' })
    await userEvent.click(eggs)
    await waitFor(() => expect(eggs).toHaveAttribute('aria-selected', 'true'))
    await expect(canvas.getByRole('option', { name: 'Milk' })).toHaveAttribute('aria-selected', 'true')
  }}
>
  <div style="width:360px">
    <List testid="list-multi" selection="multi" label="Shopping">
      <ListSection>
        {#each ['Milk', 'Eggs', 'Bread'] as item (item)}
          <ListRow
            testid="multi-{item}"
            title={item}
            role="option"
            check={multi.includes(item.toLowerCase())}
            selected={multi.includes(item.toLowerCase())}
            rowKey={item}
            onclick={() => {
              const k = item.toLowerCase()
              multi = multi.includes(k) ? multi.filter((m) => m !== k) : [...multi, k]
            }}
          />
        {/each}
      </ListSection>
    </List>
  </div>
</Story>

<Story
  name="Collapsible section"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const header = canvas.getByRole('button', { name: 'Recents' })
    await expect(header).toHaveAttribute('aria-expanded', 'true')
    await expect(canvas.getByText('Report.pdf')).toBeVisible()
    await userEvent.click(header)
    await waitFor(() => expect(canvas.queryByText('Report.pdf')).toBeNull())
    await expect(header).toHaveAttribute('aria-expanded', 'false')
  }}
>
  <div style="width:360px">
    <List testid="list-collapse" label="Files">
      <ListSection testid="recents" header="Recents" collapsible bind:expanded={open}>
        <ListRow testid="file-a" title="Report.pdf" role="listitem" />
        <ListRow testid="file-b" title="Notes.txt" role="listitem" />
      </ListSection>
    </List>
  </div>
</Story>

<Story
  name="Reorder, Alt+Arrow"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const names = (): string[] => canvas.getAllByRole('listitem').map((el) => el.textContent?.replace(/Reorder.*$/, '').trim() ?? '')
    await waitFor(() => expect(names()).toEqual(['Milk', 'Eggs', 'Bread', 'Cheese']))
    canvas.getByTestId('order-Milk-face').focus()
    await waitFor(() => expect(canvas.getByTestId('order-Milk-face')).toHaveFocus())
    await userEvent.keyboard('{Alt>}{ArrowDown}{/Alt}')
    await waitFor(() => expect(names()).toEqual(['Eggs', 'Milk', 'Bread', 'Cheese']))
    await expect(await canvas.findByText('Milk moved to position 2 of 4')).toBeInTheDocument()
    // The handle moves a row with the arrow keys too.
    canvas.getByTestId('order-Cheese-handle').focus()
    await waitFor(() => expect(canvas.getByTestId('order-Cheese-handle')).toHaveFocus())
    await userEvent.keyboard('{ArrowUp}')
    await waitFor(() => expect(names()).toEqual(['Eggs', 'Milk', 'Cheese', 'Bread']))
    // Focus stays on the handle, so the next arrow keeps moving the same row.
    await waitFor(() => expect(canvas.getByTestId('order-Cheese-handle')).toHaveFocus())
    await userEvent.keyboard('{ArrowUp}')
    await waitFor(() => expect(names()).toEqual(['Eggs', 'Cheese', 'Milk', 'Bread']))
  }}
>
  <div style="width:360px">
    <List testid="list-order" reorderable onreorder={reorder} onmove={drop} announcement={said} label="Shopping">
      <ListSection>
        {#each order as item (item)}
          <ListRow testid="order-{item}" title={item} role="listitem" rowKey={item} reorder />
        {/each}
      </ListSection>
    </List>
  </div>
</Story>

{#snippet trailingActions()}
  <SwipeAction testid="remove-action" title="Delete" icon="trash" role="destructive" onclick={() => (rows = rows.filter((r) => r !== 'Groceries'))} />
{/snippet}
<Story
  name="Swipe actions from the keyboard"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const tap = await canvas.findByTestId('swipe-Groceries-tap')
    tap.focus()
    await userEvent.keyboard('{Alt>}{ArrowLeft}{/Alt}')
    const del = canvas.getByTestId('remove-action')
    await waitFor(() => expect(del).toHaveFocus())
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(tap).toHaveFocus())
    await userEvent.keyboard('{ContextMenu}')
    await waitFor(() => expect(del).toHaveFocus())
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(canvas.queryByText('Groceries')).toBeNull())
  }}
>
  <div style="width:360px">
    <List testid="list-swipe" label="Notes">
      <ListSection>
        {#each rows as r (r)}
          <ListRow testid="swipe-{r}" title={r} role="listitem" rowKey={r} onclick={() => {}} swipeTrailing={r === 'Groceries' ? trailingActions : undefined} />
        {/each}
      </ListSection>
    </List>
  </div>
</Story>
