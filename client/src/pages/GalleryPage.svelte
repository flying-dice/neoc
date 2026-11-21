<!-- @page GalleryPage — living catalogue of the @agentos/ui kit, layer by layer. -->
<script lang="ts">
  import {
    ActivityIndicator,
    Badge,
    Button,
    ProgressBar,
    SegmentedControl,
    Slider,
    Text,
    Toggle
  } from '@agentos/ui/atoms'
  import {
    AppIcon,
    Alert,
    IconButton,
    IconSlider,
    ListRow,
    SearchField,
    Sheet,
    TextFieldRow
  } from '@agentos/ui/molecules'
  import { ListSection, NavigationBar, TabBar } from '@agentos/ui/organisms'
  import { ScreenTemplate } from '@agentos/ui/templates'
  import ColorSwatch from '../atoms/ColorSwatch.svelte'
  import SpecimenCard from '../molecules/SpecimenCard.svelte'
  import { apps } from '../lib/apps.svelte'

  type Preview = 'system' | 'light' | 'dark'
  let preview = $state<Preview>('system')

  const colors = [
    'blue',
    'green',
    'indigo',
    'orange',
    'pink',
    'purple',
    'red',
    'teal',
    'yellow',
    'mint',
    'cyan',
    'brown'
  ]
  const grays = ['gray', 'gray2', 'gray3', 'gray4', 'gray5', 'gray6']
  const textStyles = [
    'largeTitle',
    'title1',
    'title2',
    'title3',
    'headline',
    'body',
    'callout',
    'subheadline',
    'footnote',
    'caption1',
    'caption2'
  ] as const

  let toggleA = $state(true)
  let toggleB = $state(false)
  let segment = $state('day')
  let slider = $state(40)
  let volume = $state(70)
  let search = $state('')
  let tab = $state('agents')
  let sheetOpen = $state(false)
  let alertOpen = $state(false)
  let wifi = $state(true)
  let agentName = $state('')
</script>

<div class="gallery" data-theme={preview === 'system' ? undefined : preview}>
  <ScreenTemplate>
    {#snippet navigationBar()}
      <NavigationBar testid="gallery-nav" title="UI Kit" large>
        {#snippet accessory()}
          <SegmentedControl
            testid="gallery-appearance"
            label="Preview appearance"
            bind:value={preview}
            segments={[
              { value: 'system' as Preview, label: 'System' },
              { value: 'light' as Preview, label: 'Light' },
              { value: 'dark' as Preview, label: 'Dark' }
            ]}
          />
        {/snippet}
      </NavigationBar>
    {/snippet}

    {#snippet content()}
      <div class="layer"><Text style="footnote" color="secondary">TOKENS</Text></div>
      <div class="grid">
        <SpecimenCard title="System Colors" caption="Adapt to light & dark automatically">
          {#each colors as c (c)}<ColorSwatch
              testid="gallery-color-{c}"
              color="var(--ios-{c})"
              label={c}
            />{/each}
        </SpecimenCard>
        <SpecimenCard title="Grays">
          {#each grays as g (g)}<ColorSwatch
              testid="gallery-color-{g}"
              color="var(--ios-{g})"
              label={g}
            />{/each}
        </SpecimenCard>
        <SpecimenCard title="Typography" caption="Dynamic Type — Large">
          <div class="stack">
            {#each textStyles as s (s)}<Text style={s}>{s}</Text>{/each}
          </div>
        </SpecimenCard>
      </div>

      <div class="layer"><Text style="footnote" color="secondary">ATOMS</Text></div>
      <div class="grid">
        <SpecimenCard title="Button" caption="filled · tinted · gray · plain">
          <Button testid="gallery-button-filled">Filled</Button>
          <Button testid="gallery-button-tinted" variant="tinted">Tinted</Button>
          <Button testid="gallery-button-gray" variant="gray">Gray</Button>
          <Button testid="gallery-button-plain" variant="plain">Plain</Button>
          <Button testid="gallery-button-capsule" size="small" capsule>Capsule</Button>
          <Button testid="gallery-button-large-block" size="large" block>Large Block</Button>
          <Button testid="gallery-button-destructive" variant="tinted" destructive
            >Destructive</Button
          >
          <Button testid="gallery-button-disabled" disabled>Disabled</Button>
        </SpecimenCard>
        <SpecimenCard title="Toggle & SegmentedControl">
          <Toggle testid="gallery-toggle-a" bind:checked={toggleA} label="Toggle A" />
          <Toggle testid="gallery-toggle-b" bind:checked={toggleB} label="Toggle B" />
          <Toggle testid="gallery-toggle-disabled" checked disabled label="Disabled" />
          <SegmentedControl
            testid="gallery-range"
            block
            label="Range"
            bind:value={segment}
            segments={[
              { value: 'day', label: 'Day' },
              { value: 'week', label: 'Week' },
              { value: 'month', label: 'Month' },
              { value: 'year', label: 'Year' }
            ]}
          />
        </SpecimenCard>
        <SpecimenCard title="Slider · Progress · Activity">
          <Slider testid="gallery-slider" bind:value={slider} label="Slider" />
          <ProgressBar value={slider / 100} label="Progress" />
          <ProgressBar label="Indeterminate" />
          <ActivityIndicator />
          <ActivityIndicator size="large" />
          <Badge value={3} />
          <Badge value={128} />
          <Badge />
        </SpecimenCard>
      </div>

      <div class="layer"><Text style="footnote" color="secondary">MOLECULES</Text></div>
      <div class="grid">
        <SpecimenCard title="IconButton">
          <IconButton
            testid="gallery-iconbutton-new"
            icon="plus"
            title="New Agent"
            variant="filled"
          />
          <IconButton
            testid="gallery-iconbutton-run"
            icon="play"
            title="Run"
            variant="tinted"
            capsule
          />
          <IconButton
            testid="gallery-iconbutton-delete"
            icon="trash"
            label="Delete"
            variant="gray"
            destructive
          />
          <IconButton testid="gallery-iconbutton-more" icon="ellipsis" label="More" />
        </SpecimenCard>
        <SpecimenCard title="SearchField · IconSlider">
          <SearchField testid="gallery-search" bind:value={search} showCancel />
          <IconSlider
            testid="gallery-brightness"
            bind:value={volume}
            minIcon="moon"
            maxIcon="sun"
            label="Brightness"
          />
        </SpecimenCard>
        <SpecimenCard title="AppIcon" caption="Squircle, badge">
          <div class="wallpaper">
            {#each Object.values(apps) as app, i (app.id)}
              <AppIcon
                testid="gallery-appicon-{app.id}"
                name={app.name}
                icon={app.icon}
                background={app.background}
                size={56}
                badge={i === 0 ? 2 : undefined}
              />
            {/each}
          </div>
        </SpecimenCard>
        <SpecimenCard title="Sheet & Alert" caption="Modal presentations">
          <Button testid="gallery-present-sheet" variant="tinted" onclick={() => (sheetOpen = true)}
            >Present Sheet</Button
          >
          <Button
            testid="gallery-show-alert"
            variant="tinted"
            destructive
            onclick={() => (alertOpen = true)}>Show Alert</Button
          >
        </SpecimenCard>
      </div>

      <div class="layer"><Text style="footnote" color="secondary">ORGANISMS</Text></div>
      <div class="grid wide">
        <SpecimenCard title="ListSection" caption="Inset grouped">
          <div class="flush">
            <ListSection
              header="Connectivity"
              footer="Rows compose IconTile, Icon, Toggle and chevrons."
            >
              <ListRow
                testid="gallery-row-wifi"
                title="Wi-Fi"
                icon="wifi"
                iconColor="var(--ios-blue)"
                value={wifi ? 'Home' : 'Off'}
                chevron
                onclick={() => {}}
              />
              <ListRow
                testid="gallery-row-agents-online"
                title="Agents Online"
                icon="sparkles"
                iconColor="var(--ios-purple)"
              >
                {#snippet trailing()}<Toggle
                    testid="gallery-agents-online"
                    bind:checked={wifi}
                    label="Agents online"
                  />{/snippet}
              </ListRow>
              <ListRow
                testid="gallery-row-sign-out"
                title="Sign Out"
                destructive
                onclick={() => {}}
              />
            </ListSection>
          </div>
        </SpecimenCard>
        <SpecimenCard title="NavigationBar & TabBar">
          <div class="device">
            <NavigationBar testid="gallery-device-nav" title="Inbox" backLabel="Agents" material>
              {#snippet trailing()}<IconButton
                  testid="gallery-device-compose"
                  icon="square.and.pencil"
                  label="Compose"
                />{/snippet}
            </NavigationBar>
            <div class="device-body"></div>
            <TabBar
              testid="gallery-tabbar"
              bind:value={tab}
              tabs={[
                { value: 'agents', label: 'Agents', icon: 'sparkles', badge: 2 },
                { value: 'files', label: 'Files', icon: 'folder' },
                { value: 'activity', label: 'Activity', icon: 'chart.bar' },
                { value: 'settings', label: 'Settings', icon: 'gear' }
              ]}
            />
          </div>
        </SpecimenCard>
      </div>
    {/snippet}
  </ScreenTemplate>

  <Sheet testid="gallery-sheet" bind:open={sheetOpen} label="New Agent" width={480}>
    <NavigationBar testid="gallery-sheet-nav" title="New Agent">
      {#snippet leading()}
        <Button testid="gallery-sheet-cancel" variant="plain" onclick={() => (sheetOpen = false)}
          >Cancel</Button
        >
      {/snippet}
      {#snippet trailing()}
        <Button
          testid="gallery-sheet-create"
          variant="plain"
          disabled={!agentName}
          onclick={() => (sheetOpen = false)}
        >
          <strong>Create</strong>
        </Button>
      {/snippet}
    </NavigationBar>
    <div class="sheet-body">
      <ListSection header="Details">
        <TextFieldRow
          testid="gallery-sheet-name"
          label="Name"
          bind:value={agentName}
          placeholder="Research Assistant"
        />
        <ListRow
          testid="gallery-sheet-model"
          title="Model"
          value="Claude Opus 5.5"
          chevron
          onclick={() => {}}
        />
      </ListSection>
    </div>
  </Sheet>

  <Alert
    testid="gallery-alert"
    bind:open={alertOpen}
    title="Stop All Agents?"
    message="Running tasks will be cancelled."
    actions={[
      { label: 'Cancel', style: 'cancel' },
      { label: 'Stop All', style: 'destructive' }
    ]}
  />
</div>

<style>
  .gallery {
    height: 100%;
    color: var(--label);
  }
  .layer {
    padding: 8px 24px 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(300px, 100%), 1fr));
    align-items: start;
    gap: 16px;
    padding: 0 20px 24px;
  }
  .grid.wide {
    grid-template-columns: repeat(auto-fill, minmax(min(360px, 100%), 1fr));
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .wallpaper {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
    width: 100%;
    padding: 18px;
    border-radius: 14px;
    background: linear-gradient(135deg, #6e8efb, #c471ed 60%, #f7797d);
  }
  .flush {
    width: 100%;
    --list-inset: 0;
    --list-row-bg: var(--bg-grouped-elevated-secondary);
  }
  .flush :global(.ios-list-section) {
    margin-bottom: 0;
  }
  .device {
    width: 100%;
    height: 260px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 14px;
    background: var(--bg-grouped-elevated);
    box-shadow: inset 0 0 0 0.5px var(--separator);
  }
  .device-body {
    flex: 1;
  }
  .sheet-body {
    padding: 8px 0 12px;
  }
</style>
