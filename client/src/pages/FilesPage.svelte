<!-- @page FilesPage — workspace file browser (placeholder data). -->
<script lang="ts">
  import { ListRow, SearchField, IconButton } from '@vos/ui/molecules'
  import { ListSection, NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate } from '@vos/ui/templates'

  let query = $state('')

  const recents = [
    { name: 'agent-memory-survey.md', meta: 'Research · 2 min ago' },
    { name: 'refactor-plan.md', meta: 'Coder · 1 hr ago' },
    { name: 'inbox-digest.txt', meta: 'Inbox · Yesterday' },
    { name: 'uptime-report.csv', meta: 'Ops · Monday' }
  ]
  const filtered = $derived(recents.filter((f) => f.name.includes(query.toLowerCase())))
</script>

<ScreenTemplate>
  {#snippet navigationBar()}
    <NavigationBar testid="files-nav" title="Files" large>
      {#snippet trailing()}
        <IconButton testid="files-more" icon="ellipsis" label="More" />
      {/snippet}
      {#snippet accessory()}
        <SearchField testid="files-search" bind:value={query} />
      {/snippet}
    </NavigationBar>
  {/snippet}

  {#snippet content()}
    <div class="body">
      <ListSection header="Locations">
        <ListRow
          testid="files-location-drive"
          title="AgentOS Drive"
          icon="folder"
          iconColor="var(--ios-blue)"
          chevron
          onclick={() => {}}
        />
        <ListRow
          testid="files-location-computer"
          title="On This Computer"
          icon="house"
          iconColor="var(--ios-gray)"
          chevron
          onclick={() => {}}
        />
        <ListRow
          testid="files-location-deleted"
          title="Recently Deleted"
          icon="trash"
          iconColor="var(--ios-red)"
          chevron
          onclick={() => {}}
        />
      </ListSection>
      <ListSection header="Recents">
        {#each filtered as f (f.name)}
          <ListRow
            testid="files-recent-{f.name}"
            title={f.name}
            subtitle={f.meta}
            icon="doc"
            iconColor="var(--ios-teal)"
            chevron
            onclick={() => {}}
          />
        {/each}
      </ListSection>
    </div>
  {/snippet}
</ScreenTemplate>

<style>
  .body {
    padding-top: 8px;
  }
</style>
