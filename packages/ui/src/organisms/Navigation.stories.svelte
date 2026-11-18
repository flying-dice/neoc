<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import Button from '../atoms/Button.svelte'
  import Text from '../atoms/Text.svelte'
  import ListRow from '../molecules/ListRow.svelte'
  import ScreenTemplate from '../templates/ScreenTemplate.svelte'
  import List from './List.svelte'
  import ListSection from './ListSection.svelte'
  import NavigationBar from './NavigationBar.svelte'
  import NavigationStack from './NavigationStack.svelte'
  import TabView from './TabView.svelte'
  import VirtualRows from './VirtualRows.svelte'

  const { Story } = defineMeta({
    title: 'Organisms/Navigation',
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })

  const box = 'width:360px;height:420px;border-radius:14px;overflow:hidden;background:var(--bg-grouped-elevated)'
</script>

<script lang="ts">
  let items = $state([{ id: 'root', leaving: false }])
  let n = 0
  const titles: Record<string, string> = { root: 'Inbox' }
  function push(): void {
    const id = `s${++n}`
    titles[id] = `Note ${n}`
    items = [...items, { id, leaving: false }]
  }
  function pop(): void {
    const top = [...items].reverse().find((i) => !i.leaving)
    if (top && top.id !== 'root') items = items.map((i) => (i === top ? { ...i, leaving: true } : i))
  }
  const live = $derived(items.filter((i) => !i.leaving))

  let tab = $state('inbox')
  const tabs = [
    { id: 'inbox', label: 'Inbox', icon: 'message' as const, badge: 3 },
    { id: 'files', label: 'Files', icon: 'folder' as const },
    { id: 'settings', label: 'Settings', icon: 'gear' as const }
  ]
</script>

{#snippet screen(id: string)}
  {@const depth = live.findIndex((i) => i.id === id)}
  <ScreenTemplate testid="nav-{id}">
    {#snippet navigationBar()}
      <NavigationBar
        testid="nav-{id}-bar"
        title={titles[id] ?? 'Note'}
        large={id === 'root'}
        backLabel={depth > 0 ? (titles[live[depth - 1].id] ?? 'Back') : undefined}
        onback={pop}
      />
    {/snippet}
    {#snippet content()}
      <div style="padding:16px;display:grid;gap:12px;justify-items:start">
        <Text>Screen {id}</Text>
        <Button testid="nav-{id}-push" variant="tinted" onclick={push}>Push another</Button>
      </div>
    {/snippet}
  </ScreenTemplate>
{/snippet}

<!-- A navigation stack: the top screen slides over the others (inert, hidden from assistive technology); focus moves to the new title and back. -->
<Story
  name="NavigationStack, push and pop"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('nav-root-push'))
    const title = await canvas.findByRole('heading', { name: 'Note 1' })
    await waitFor(() => expect(title).toHaveFocus())
    const back = await canvas.findByRole('button', { name: 'Inbox' })
    await userEvent.click(back)
    await waitFor(() => expect(canvas.queryByRole('heading', { name: 'Note 1' })).toBeNull(), { timeout: 3000 })
    await waitFor(() => expect(canvas.getByTestId('nav-root-push')).toHaveFocus())
  }}
>
  <div style={box}>
    <NavigationStack testid="stack" {items} {screen} onleft={(id) => (items = items.filter((i) => i.id !== id))} />
  </div>
</Story>

<Story name="TabView, bottom" asChild parameters={{ controls: { disable: true } }}>
  <div style={box}>
    <TabView testid="tabs-bottom" {tabs} selected={tab} onselect={(id) => (tab = id)}>
      {#snippet panel(id)}<div style="padding:16px"><Text>Panel: {id}</Text></div>{/snippet}
    </TabView>
  </div>
</Story>

<Story
  name="TabView, keyboard and roles"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByRole('tablist')).toBeVisible()
    const [inbox, files] = canvas.getAllByRole('tab')
    await expect(inbox).toHaveAttribute('aria-selected', 'true')
    await expect(inbox).toHaveAttribute('tabindex', '0')
    await expect(files).toHaveAttribute('tabindex', '-1')
    inbox.focus()
    await userEvent.keyboard('{ArrowRight}')
    await waitFor(() => expect(files).toHaveFocus())
    await expect(files).toHaveAttribute('aria-selected', 'true')
    await expect(canvas.getByRole('tabpanel', { name: 'Files' })).toBeVisible()
    await userEvent.keyboard('{End}')
    await expect(canvas.getAllByRole('tab')[2]).toHaveAttribute('aria-selected', 'true')
  }}
>
  <div style={box}>
    <TabView testid="tabs-kbd" {tabs} selected={tab} onselect={(id) => (tab = id)}>
      {#snippet panel(id)}<div style="padding:16px"><Text>Panel: {id}</Text></div>{/snippet}
    </TabView>
  </div>
</Story>

<Story name="TabView, top" asChild parameters={{ controls: { disable: true } }}>
  <div style={box}>
    <TabView testid="tabs-top" style="top" {tabs} selected="files">
      {#snippet panel(id)}<div style="padding:16px"><Text>Panel: {id}</Text></div>{/snippet}
    </TabView>
  </div>
</Story>

<Story name="TabView, sidebar" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:420px;height:300px;border-radius:14px;overflow:hidden">
    <TabView testid="tabs-side" style="sidebar" {tabs} selected="settings">
      {#snippet panel(id)}<div style="padding:16px"><Text>Panel: {id}</Text></div>{/snippet}
    </TabView>
  </div>
</Story>

<Story name="TabView, page" asChild parameters={{ controls: { disable: true } }}>
  <div style={box}>
    <TabView testid="tabs-page" style="page" {tabs} selected="inbox">
      {#snippet panel(id)}<div style="padding:16px"><Text>Page: {id}</Text></div>{/snippet}
    </TabView>
  </div>
</Story>

<!-- 100 000 rows, a few dozen in the DOM. The list scrolls itself here (no scrolling ancestor). -->
<Story
  name="VirtualRows, 100,000 rows"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, canvasElement }) => {
    await canvas.findByText('Row 1')
    const rendered = (): number => canvasElement.querySelectorAll('[data-testid^="virtual-row-"]').length
    await waitFor(() => expect(rendered()).toBeGreaterThan(5))
    await expect(rendered()).toBeLessThan(40)
    const host = canvasElement.querySelector<HTMLElement>('[data-testid="virtual"]')!
    host.scrollTop = 44 * 60_000
    await canvas.findAllByText(/^Row [4-9][0-9]{4}$/)
    await expect(rendered()).toBeLessThan(40)
  }}
>
  <div style="width:360px;height:300px;border-radius:14px;overflow:hidden;background:var(--bg-elevated)">
    <List testid="big" label="A hundred thousand rows">
      <ListSection>
        <VirtualRows testid="virtual" total={100_000} estimate={44}>
          {#snippet row(i)}
            <ListRow testid="virtual-row-{i}" title="Row {i + 1}" role="listitem" posinset={i + 1} setsize={100_000} />
          {/snippet}
        </VirtualRows>
      </ListSection>
    </List>
  </div>
</Story>
