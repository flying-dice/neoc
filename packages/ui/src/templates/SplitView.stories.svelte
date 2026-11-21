<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import Text from '../atoms/Text.svelte'
  import ListRow from '../molecules/ListRow.svelte'
  import List from '../organisms/List.svelte'
  import ListSection from '../organisms/ListSection.svelte'
  import NavigationBar from '../organisms/NavigationBar.svelte'
  import ScreenTemplate from './ScreenTemplate.svelte'
  import SplitViewTemplate from './SplitViewTemplate.svelte'

  const { Story } = defineMeta({
    title: 'Templates/SplitView and Screen',
    component: SplitViewTemplate,
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })
</script>

<script lang="ts">
  let folder = $state('notes')
  let compact = $state<'sidebar' | 'content' | 'detail'>('sidebar')
  let refreshed = $state(0)
</script>

{#snippet sidebar()}
  <ScreenTemplate>
    {#snippet navigationBar()}<NavigationBar testid="sb-bar" title="Folders" />{/snippet}
    {#snippet content()}
      <List testid="sb-list" style="sidebar" selection="single" label="Folders">
        <ListSection>
          <ListRow testid="sb-notes" title="Notes" icon="doc" role="option" selected={folder === 'notes'} onclick={() => ((folder = 'notes'), (compact = 'content'))} />
          <ListRow testid="sb-reminders" title="Reminders" icon="checkmark" role="option" selected={folder === 'reminders'} onclick={() => ((folder = 'reminders'), (compact = 'content'))} />
        </ListSection>
      </List>
    {/snippet}
  </ScreenTemplate>
{/snippet}
{#snippet content()}
  <ScreenTemplate background="plain">
    {#snippet navigationBar()}<NavigationBar testid="ct-bar" title={folder === 'notes' ? 'Notes' : 'Reminders'} backLabel={compact === 'content' ? 'Folders' : undefined} onback={() => (compact = 'sidebar')} />{/snippet}
    {#snippet content()}<div style="padding:16px"><Text>The {folder} list</Text></div>{/snippet}
  </ScreenTemplate>
{/snippet}
{#snippet detail()}
  <ScreenTemplate background="plain">
    {#snippet navigationBar()}<NavigationBar testid="dt-bar" title="Detail" />{/snippet}
    {#snippet content()}<div style="padding:16px"><Text>The detail pane</Text></div>{/snippet}
  </ScreenTemplate>
{/snippet}

<Story
  name="Three columns"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('split-sidebar')).toBeVisible()
    await expect(canvas.getByTestId('split-content')).toBeVisible()
    await expect(canvas.getByTestId('split-detail')).toBeVisible()
  }}
>
  <div style="width:1000px;height:360px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate testid="split" {sidebar} {content} {detail} />
  </div>
</Story>

<Story name="Two columns" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:760px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate testid="split2" {sidebar} detail={content} />
  </div>
</Story>

<!-- Narrow three columns: the sidebar is hidden first (`double`), the detail keeps the room. -->
<Story
  name="Three columns, narrow window hides the sidebar"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.queryByTestId('split-narrow-sidebar')).toBeNull())
    await expect(canvas.getByTestId('split-narrow-content')).toBeVisible()
  }}
>
  <div style="width:800px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate testid="split-narrow" {sidebar} {content} {detail} />
  </div>
</Story>

<Story
  name="Collapsed, one column at a time"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await waitFor(() => expect(canvas.getByTestId('collapsed-sidebar')).toBeVisible())
    await waitFor(() => expect(canvas.getByTestId('collapsed-detail')).not.toBeVisible())
    await userEvent.click(canvas.getByTestId('sb-reminders'))
    await waitFor(() => expect(canvas.getByTestId('collapsed-content')).toBeVisible())
    await userEvent.click(await canvas.findByRole('button', { name: /Folders/ }))
    await waitFor(() => expect(canvas.getByTestId('collapsed-sidebar')).toBeVisible())
  }}
>
  <div style="width:420px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate testid="collapsed" collapseAt={700} keepMounted compactColumn={compact} {sidebar} {content} {detail} />
  </div>
</Story>

<!-- Screen with refresh: the keyboard "Refresh" button appears on focus; the spinner runs until the callback resolves. -->
<Story
  name="Screen, pull to refresh"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const key = canvas.getByTestId('screen-refresh')
    key.focus()
    await expect(key).toBeVisible()
    await userEvent.click(key)
    await waitFor(() => expect(canvas.getByText(/Refreshed 1 time/)).toBeVisible())
  }}
>
  <div style="width:360px;height:300px;border-radius:14px;overflow:hidden">
    <ScreenTemplate testid="screen" onrefresh={() => new Promise((r) => setTimeout(() => ((refreshed += 1), r(undefined)), 300))}>
      {#snippet navigationBar()}<NavigationBar testid="rf-bar" title="Feed" large />{/snippet}
      {#snippet content()}<div style="padding:16px"><Text>Refreshed {refreshed} time{refreshed === 1 ? '' : 's'}</Text></div>{/snippet}
    </ScreenTemplate>
  </div>
</Story>
