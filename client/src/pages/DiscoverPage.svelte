<!--
  @page DiscoverPage — the landing window: a store-style "Discover" app in an
  iPadOS split view, introducing vos and the apps you can install.
-->
<script lang="ts">
  import type { IconName } from '@vos/ui'
  import { Icon, IconTile, Text } from '@vos/ui/atoms'
  import { ListRow, SearchField } from '@vos/ui/molecules'
  import { ListSection, NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate, SplitViewTemplate } from '@vos/ui/templates'
  import AppListing from '../molecules/AppListing.svelte'
  import { windowManager as wm } from '../lib/windowManager.svelte'
  import { host } from '../lib/host.svelte'
  import { messageOf } from '../shell/errors'
  import {
    iconOf,
    tintBackground,
    type AppPreview,
    type AppState,
    type AppSummary,
    type Catalog
  } from '../shell/hostApi'

  /** One row of the Apps section: an installed app, or one a registry offers. */
  interface Listing {
    id: string
    name: string
    detail: string
    description?: string
    icon?: string
    tint?: string
    state?: AppState
    message?: string
    /** The app-ref URL to install from (not installed yet). */
    url?: string
    /** Its digest as the catalog listed it, so the install is exactly the ref that was shown. */
    sha256?: string
  }

  let catalog = $state<Catalog>({ apps: [], errors: [] })
  let busy = $state<Record<string, boolean>>({})
  let storeError = $state<string | null>(null)

  async function loadCatalog(): Promise<void> {
    try {
      catalog = await host.catalog()
    } catch (e) {
      storeError = messageOf(e)
    }
  }
  // The catalog says which apps are installed: read it again whenever the installed set changes.
  $effect(() => {
    void host.apps
    void loadCatalog()
  })

  const fromInstalled = (a: AppSummary): Listing => ({
    id: a.id,
    name: a.name,
    detail: `${a.version} · ${a.source}`,
    description: a.description,
    icon: a.icon,
    tint: a.tint,
    state: a.state,
    message: a.message
  })
  const fromCatalog = (a: AppPreview): Listing => ({
    id: a.id,
    name: a.name,
    detail: `${a.version} · ${a.source}`,
    description: a.description,
    icon: a.icon,
    tint: a.tint,
    url: a.url,
    sha256: a.sha256
  })

  const listings = $derived<Listing[]>(
    [
      ...host.apps.map(fromInstalled),
      ...catalog.apps.filter((a) => !host.apps.some((i) => i.id === a.id)).map(fromCatalog)
    ].filter((l) => l.name.toLowerCase().includes(query.toLowerCase()))
  )

  async function run(key: string, op: () => Promise<unknown>): Promise<boolean> {
    if (busy[key]) return false
    busy[key] = true
    storeError = null
    try {
      await op()
      await host.refresh()
      return true
    } catch (e) {
      storeError = messageOf(e)
      return false
    } finally {
      delete busy[key]
    }
  }

  async function primary(l: Listing): Promise<void> {
    const call = (method: string, params: Record<string, unknown>): Promise<unknown> =>
      host.api.call(method, params)
    let done = false
    if (l.url) done = await run(l.id, () => call('app.install', { url: l.url, sha256: l.sha256 }))
    else if (l.state === 'running') done = true
    else if (l.state === 'disabled') done = await run(l.id, () => call('app.enable', { app: l.id }))
    else if (l.state === 'failed') done = await run(l.id, () => call('app.reload', { app: l.id }))
    if (done) wm.open(l.id)
  }

  const toggle = (l: Listing): Promise<boolean> =>
    run(l.id, () =>
      host.api.call(l.state === 'running' ? 'app.disable' : 'app.enable', { app: l.id })
    )
  const remove = (l: Listing): Promise<boolean> =>
    run(l.id, () => host.api.call('app.uninstall', { app: l.id }))

  /** What an installed app can do, said before one is installed: an app is the person's own code on their own host. */
  const accessNote =
    'Apps can read and write files, use the network and run processes on this machine.'

  /** The sidebar's rows: the front page, or the apps alone. */
  type Section = 'discover' | 'apps'

  const sections: { id: Section; label: string; icon: IconName }[] = [
    { id: 'discover', label: 'Discover', icon: 'star' },
    { id: 'apps', label: 'Apps', icon: 'square.grid' }
  ]

  let section = $state<Section>('discover')
  let query = $state('')

  /** The tiles floating in the hero: decoration only (they stand for no app). */
  const heroTiles = [
    { icon: 'globe', background: 'linear-gradient(180deg,#5ac8fa,#007aff)' },
    { icon: 'terminal', background: 'linear-gradient(180deg,#7d7aff,#4b45d6)' },
    { icon: 'message', background: 'linear-gradient(180deg,#5fe08a,#20b04b)' },
    { icon: 'cpu', background: 'linear-gradient(180deg,#ffb340,#ff7a00)' }
  ] satisfies { icon: IconName; background: string }[]

  const information = $derived<[string, string][]>([
    [
      'Apps Running',
      `${host.apps.filter((a) => a.state === 'running').length} of ${host.apps.length}`
    ],
    ['Apps Offered', String(catalog.apps.filter((a) => !a.installed).length)]
  ])
</script>

<SplitViewTemplate sidebarWidth={280}>
  {#snippet sidebar()}
    <NavigationBar testid="discover-sidebar-nav" title="Discover" large>
      {#snippet accessory()}
        <SearchField testid="discover-search" bind:value={query} />
      {/snippet}
    </NavigationBar>
    <ListSection sidebar>
      {#each sections as s (s.id)}
        <ListRow
          testid="discover-section-{s.id}"
          title={s.label}
          icon={s.icon}
          iconColor="var(--tint)"
          selected={section === s.id}
          onclick={() => (section = s.id)}
        />
      {/each}
    </ListSection>
  {/snippet}

  {#snippet detail()}
    <ScreenTemplate background="plain">
      {#snippet navigationBar()}
        <NavigationBar
          testid="discover-nav"
          title={sections.find((s) => s.id === section)?.label ?? 'Discover'}
        />
      {/snippet}

      {#snippet content()}
        <div class="page">
          {#if section === 'discover'}
            <section class="hero" data-testid="discover-hero">
              <div class="hero-copy">
                <h1>Find and install apps</h1>
                <p>
                  Customise your system with the apps you want, and remove them whenever you like.
                </p>
              </div>
              <div class="hero-art" aria-hidden="true">
                {#each heroTiles as tile, i (tile.icon)}
                  <span class="float f{i}">
                    <IconTile size={84 - i * 8} background={tile.background} shadow>
                      <Icon name={tile.icon} size={44 - i * 4} weight={1.7} color="#fff" />
                    </IconTile>
                  </span>
                {/each}
              </div>
            </section>
          {/if}

          <section data-testid="discover-apps">
            <header class="section-head">
              <Text style="title3" weight="bold" as="h2">Apps</Text>
            </header>
            {#if storeError}
              <p class="store-error" role="alert" data-testid="discover-apps-error">
                {storeError}
              </p>
            {/if}
            <div class="listings">
              {#each listings as l (l.id)}
                <AppListing
                  testid="discover-app-{l.id}"
                  name={l.name}
                  detail={l.detail}
                  description={l.description}
                  icon={iconOf(l.icon)}
                  background={tintBackground(l.tint)}
                  state={l.state}
                  message={l.message}
                  busy={!!busy[l.id]}
                  onget={() => void primary(l)}
                  ontoggle={() => void toggle(l)}
                  onremove={() => void remove(l)}
                />
              {:else}
                <Text style="subheadline" color="secondary">
                  {query
                    ? `No apps match “${query}”.`
                    : 'No apps are available yet. Build the bundled apps with `bun run build:apps`.'}
                </Text>
              {/each}
            </div>
            {#if listings.some((l) => l.url)}
              <p class="access-note" data-testid="discover-apps-access-note">
                <Text style="footnote" color="secondary"
                  >{accessNote} Only install apps you trust.</Text
                >
              </p>
            {/if}
            {#if host.appsError}
              <p class="store-error" role="alert" data-testid="discover-apps-list-error">
                {host.appsError}
              </p>
            {/if}
            {#each catalog.errors as problem (problem)}
              <p class="store-error" role="status" data-testid="discover-registry-error">
                {problem}
              </p>
            {/each}
          </section>

          {#if section === 'discover'}
            <hr />

            <section data-testid="discover-information">
              <Text style="title3" weight="bold" as="h2">Information</Text>
              <dl class="info">
                {#each information as [k, v] (k)}
                  <div>
                    <dt><Text style="caption1" color="secondary">{k}</Text></dt>
                    <dd><Text style="footnote">{v}</Text></dd>
                  </div>
                {/each}
              </dl>
            </section>
          {/if}
        </div>
      {/snippet}
    </ScreenTemplate>
  {/snippet}
</SplitViewTemplate>

<style>
  .page {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 26px;
    max-width: 980px;
    margin: 0 auto;
    padding: 24px min(32px, 5%) 120px;
  }
  .hero {
    position: relative;
    display: flex;
    align-items: center;
    min-height: 260px;
    padding: 34px 36px;
    border-radius: 18px;
    overflow: hidden;
    color: #fff;
    /* Flat: one solid brand colour (white text is 7.6:1 on it). */
    background: #4b3bd6;
  }
  .hero-copy {
    position: relative;
    z-index: 1;
    max-width: min(420px, calc(100% - 300px));
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hero h1 {
    margin: 0;
    font: 700 30px/36px var(--font-display);
    letter-spacing: 0.2px;
  }
  .hero p {
    margin: 0;
    font: var(--type-callout);
    opacity: 0.9;
  }
  .hero-art {
    position: absolute;
    right: 24px;
    top: 0;
    bottom: 0;
    width: 270px;
  }
  .float {
    position: absolute;
    animation: bob 6s ease-in-out infinite;
  }
  .f0 {
    right: 120px;
    top: 36px;
  }
  .f1 {
    right: 20px;
    top: 70px;
    animation-delay: -1.5s;
  }
  .f2 {
    right: 170px;
    top: 150px;
    animation-delay: -3s;
  }
  .f3 {
    right: 60px;
    top: 172px;
    animation-delay: -4.5s;
  }
  @keyframes bob {
    50% {
      transform: translateY(-8px) rotate(-2deg);
    }
  }

  .access-note {
    margin: 12px 0 8px;
  }
  .store-error {
    margin: 0 0 12px;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 14px;
  }
  .listings {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
    gap: 22px 24px;
  }
  hr {
    width: 100%;
    margin: 0;
    border: 0;
    height: 0.5px;
    background: var(--separator);
  }
  .info {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 14px 24px;
    margin: 12px 0 0;
  }
  dt,
  dd {
    margin: 0;
  }

  /* Narrow windows (Slide Over, split, small floating): stack the hero, drop the art. */
  @container (width < 640px) {
    .hero {
      min-height: 0;
      padding: 24px;
    }
    .hero-art {
      display: none;
    }
    .hero-copy {
      max-width: none;
    }
    .hero h1 {
      font: var(--type-title2);
      font-weight: 700;
    }
  }
</style>
