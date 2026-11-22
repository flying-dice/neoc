<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import Text from '../atoms/Text.svelte'
  import ListRow from '../molecules/ListRow.svelte'
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
  // the collapsed navigation flow: the list first, a row shows its detail, Back returns to the list
  let browsing = $state(true)
  let collapsed = $state(false)
</script>

{#snippet sidebar()}
  <ScreenTemplate>
    {#snippet navigationBar()}<NavigationBar testid="sb-bar" title="Folders" />{/snippet}
    {#snippet content()}
      <ListSection sidebar>
        <ListRow testid="sb-notes" title="Notes" icon="doc" selected={folder === 'notes'} onclick={() => (folder = 'notes')} />
        <ListRow testid="sb-reminders" title="Reminders" icon="checkmark" selected={folder === 'reminders'} onclick={() => (folder = 'reminders')} />
      </ListSection>
    {/snippet}
  </ScreenTemplate>
{/snippet}
{#snippet detail()}
  <ScreenTemplate background="plain">
    {#snippet navigationBar()}<NavigationBar testid="dt-bar" title={folder === 'notes' ? 'Notes' : 'Reminders'} />{/snippet}
    {#snippet content()}<div style="padding:16px"><Text>The {folder} list</Text></div>{/snippet}
  </ScreenTemplate>
{/snippet}

<Story
  name="Two columns"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('sb-reminders'))
    await waitFor(() => expect(canvas.getByText('The reminders list')).toBeVisible())
    await expect(canvas.getByTestId('sb-reminders')).toHaveAttribute('aria-current', 'true')
  }}
>
  <div style="width:760px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate {sidebar} {detail} />
  </div>
</Story>

<!-- A window narrower than 560 px: the detail alone. -->
<Story
  name="Collapsed to the detail"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.queryByTestId('sb-bar')).toBeNull())
    await expect(canvas.getByText('The notes list')).toBeVisible()
  }}
>
  <div style="width:420px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate {sidebar} {detail} />
  </div>
</Story>

<!--
  Collapsed with `compact`: the sidebar is the list, choosing a row shows the detail with a Back item in its bar
  (NavigationBar `backLabel` / `onback`), and Back returns to the list.
-->
<Story
  name="Collapsed, sidebar then detail"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await waitFor(() => expect(canvas.getByTestId('nav-collapsed')).toHaveTextContent('collapsed'))
    await expect(canvas.queryByTestId('nav-dt-bar')).toBeNull()
    await userEvent.click(canvas.getByTestId('nav-reminders'))
    await waitFor(() => expect(canvas.getByText('The reminders list')).toBeVisible())
    await expect(canvas.queryByTestId('nav-reminders')).toBeNull()
    const back = canvas.getByTestId('nav-dt-bar-back')
    await expect(back).toHaveAccessibleName('Folders')
    back.focus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(canvas.getByTestId('nav-reminders')).toHaveAttribute('aria-current', 'true'))
    await expect(canvas.queryByTestId('nav-dt-bar')).toBeNull()
  }}
>
  <div style="width:420px;height:320px;border-radius:14px;overflow:hidden">
    <SplitViewTemplate compact={browsing ? 'sidebar' : 'detail'} bind:collapsed>
      {#snippet sidebar()}
        <ScreenTemplate>
          {#snippet navigationBar()}<NavigationBar testid="nav-sb-bar" title="Folders" />{/snippet}
          {#snippet content()}
            <ListSection sidebar>
              <ListRow testid="nav-notes" title="Notes" icon="doc" selected={folder === 'notes'} onclick={() => ((folder = 'notes'), (browsing = false))} />
              <ListRow testid="nav-reminders" title="Reminders" icon="checkmark" selected={folder === 'reminders'} onclick={() => ((folder = 'reminders'), (browsing = false))} />
            </ListSection>
          {/snippet}
        </ScreenTemplate>
      {/snippet}
      {#snippet detail()}
        <ScreenTemplate background="plain">
          {#snippet navigationBar()}
            <NavigationBar
              testid="nav-dt-bar"
              title={folder === 'notes' ? 'Notes' : 'Reminders'}
              backLabel={collapsed ? 'Folders' : undefined}
              onback={() => (browsing = true)}
            />
          {/snippet}
          {#snippet content()}<div style="padding:16px"><Text>The {folder} list</Text></div>{/snippet}
        </ScreenTemplate>
      {/snippet}
    </SplitViewTemplate>
  </div>
  <p data-testid="nav-collapsed" style="font:var(--type-footnote);color:var(--label-secondary)">{collapsed ? 'collapsed' : 'two columns'}</p>
</Story>
