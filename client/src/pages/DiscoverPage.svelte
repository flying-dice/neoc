<!--
  @page DiscoverPage — the landing window: a store-style "Discover" app in an
  iPadOS split view, introducing vos, the apps you can install and the agents on this host.
-->
<script lang="ts">
  import type { IconName } from '@vos/ui'
  import { Button, Icon, IconTile, Text } from '@vos/ui/atoms'
  import { IconButton, ListRow, SearchField } from '@vos/ui/molecules'
  import { ListSection, NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate, SplitViewTemplate } from '@vos/ui/templates'
  import AgentListing from '../molecules/AgentListing.svelte'
  import AppListing from '../molecules/AppListing.svelte'
  import { Alert } from '@vos/ui/molecules'
  import { TextInput } from '@vos/ui/atoms'
  import { windowManager as wm } from '../lib/windowManager.svelte'
  import { AGENTS_APP_ID, appOf } from '../lib/apps.svelte'
  import { host } from '../lib/host.svelte'
  import { messageOf } from '../view/errors'
  import { appPreviewSchema, callParsed } from '../shell/results'
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

  // The Agents app is an ordinary installed app: its entry points show only while it is running.
  const agentsRunning = $derived(!!appOf(AGENTS_APP_ID))
  const openAgents = (): void => wm.open(AGENTS_APP_ID)

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

  /**
   * What an installed app can do, said once for every way to install one (Get, add by URL). There is no
   * permission model to describe: an app is the person's own code on their own host.
   */
  const accessNote =
    'Apps can read and write files, use the network and run processes on this machine.'

  // ── Add by URL: preview first, then say plainly what installing means ──
  let addUrl = $state('')
  let preview = $state<AppPreview | null>(null)
  let previewError = $state<string | null>(null)
  let previewing = $state(false)
  let confirming = $state(false)

  async function showPreview(): Promise<void> {
    const url = addUrl.trim()
    if (!url || previewing) return
    previewing = true
    previewError = null
    preview = null
    try {
      preview = await callParsed(host.api, 'app.preview', { url }, appPreviewSchema)
    } catch (e) {
      previewError = messageOf(e)
    } finally {
      previewing = false
    }
  }

  async function confirmInstall(): Promise<void> {
    const p = preview
    if (!p) return
    if (await run(p.url, () => host.api.call('app.install', { url: p.url, sha256: p.sha256 }))) {
      preview = null
      addUrl = ''
      wm.open(p.id)
    }
  }

  /** What the sidebar shows: everything, or only the apps, or only the agents of this host. */
  type Section = 'discover' | 'apps' | 'agents'

  const sections: { id: Section; label: string; icon: IconName }[] = [
    { id: 'discover', label: 'Discover', icon: 'star' },
    { id: 'apps', label: 'Apps', icon: 'square.grid' },
    { id: 'agents', label: 'Agents', icon: 'sparkles' }
  ]

  let section = $state<Section>('discover')
  let query = $state('')

  /** The tiles floating in the hero: decoration only (they stand for no app or agent). */
  const heroTiles = [
    { icon: 'globe', background: 'linear-gradient(180deg,#5ac8fa,#007aff)' },
    { icon: 'terminal', background: 'linear-gradient(180deg,#7d7aff,#4b45d6)' },
    { icon: 'message', background: 'linear-gradient(180deg,#5fe08a,#20b04b)' },
    { icon: 'cpu', background: 'linear-gradient(180deg,#ffb340,#ff7a00)' }
  ] satisfies { icon: IconName; background: string }[]

  /** The agents on this host's roster (`agents.list`), narrowed by the search. */
  const roster = $derived(
    host.agents.filter((a) => `${a.name} ${a.role}`.toLowerCase().includes(query.toLowerCase()))
  )
  const showApps = $derived(section !== 'agents')
  const showAgents = $derived(section !== 'apps')

  const plural = (n: number, noun: string): string => `${n} ${noun}${n === 1 ? '' : 's'}`
  const information = $derived<[string, string][]>([
    [
      'Apps Running',
      `${host.apps.filter((a) => a.state === 'running').length} of ${host.apps.length}`
    ],
    ['Agents on this Host', String(host.agents.length)],
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
    <ListSection sidebar>
      <ListRow
        testid="discover-account"
        title="vos User"
        subtitle={plural(host.agents.length, 'agent')}
      >
        {#snippet leading()}
          <IconTile size={34} background="linear-gradient(180deg,#c7c7cc,#8e8e93)">
            <Icon name="person" size={20} color="#fff" />
          </IconTile>
        {/snippet}
      </ListRow>
    </ListSection>
  {/snippet}

  {#snippet detail()}
    <ScreenTemplate background="plain">
      {#snippet navigationBar()}
        <NavigationBar testid="discover-nav" title="vos">
          {#snippet trailing()}
            {#if agentsRunning}
              <Button testid="discover-open" size="small" capsule onclick={openAgents}>OPEN</Button>
            {/if}
            <IconButton testid="discover-share" icon="square.and.arrow.up" label="Share" />
          {/snippet}
        </NavigationBar>
      {/snippet}

      {#snippet content()}
        <div class="page">
          <section class="hero">
            <div class="hero-copy">
              <Text style="caption1" weight="semibold" color="inherit">WELCOME TO VOS</Text>
              <h1>Your agents, on one desktop.</h1>
              <p>
                Run research, code, inbox and ops agents side by side. Watch them work, step in when
                they need you, and keep everything on your machine.
              </p>
              <div class="hero-actions">
                {#if agentsRunning}
                  <Button
                    testid="discover-hero-open-agents"
                    size="large"
                    capsule
                    tint="#fff"
                    onclick={openAgents}
                  >
                    <span class="hero-cta">Open Agents</span>
                  </Button>
                {/if}
                <Button
                  testid="discover-hero-ui-kit"
                  variant="plain"
                  tint="#fff"
                  onclick={() => wm.open('gallery')}>Explore the UI Kit ›</Button
                >
              </div>
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

          {#if showApps}
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
                      : 'No apps are available yet. Add one by URL below, or build the bundled apps with `bun run build:apps`.'}
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

              <form
                class="add"
                data-testid="discover-add"
                onsubmit={(e) => {
                  e.preventDefault()
                  void showPreview()
                }}
              >
                <Text style="subheadline" weight="semibold" as="h3">Add an app by URL</Text>
                <Text style="footnote" color="secondary" as="p"
                  >Paste the address of an app ref (https:// or file://). You will see what it is
                  before anything is installed.</Text
                >
                <div class="add-row">
                  <span class="url-box">
                    <TextInput
                      testid="discover-add-url"
                      bind:value={addUrl}
                      placeholder="https://example.org/apps/notes/app-ref.json"
                      aria-label="App URL"
                      autocomplete="off"
                      spellcheck={false}
                    />
                  </span>
                  <Button
                    testid="discover-add-preview"
                    type="submit"
                    size="small"
                    capsule
                    hit
                    disabled={!addUrl.trim() || previewing}
                    >{previewing ? 'Looking…' : 'Preview'}</Button
                  >
                </div>
                {#if previewError}
                  <p class="store-error" role="alert" data-testid="discover-add-error">
                    {previewError}
                  </p>
                {/if}
                {#if preview}
                  <div class="preview" data-testid="discover-add-preview-card">
                    <IconTile size={56} background={tintBackground(preview.tint)} shadow>
                      <Icon name={iconOf(preview.icon)} size={30} weight={1.8} color="#fff" />
                    </IconTile>
                    <div class="preview-meta">
                      <Text style="headline">{preview.name}</Text>
                      <Text style="caption1" color="secondary"
                        >Version {preview.version} · from {preview.source}</Text
                      >
                      {#if preview.description}
                        <Text style="footnote">{preview.description}</Text>
                      {/if}
                      {#if preview.installed}
                        <Text style="caption1" color="secondary"
                          >Already installed (version {preview.installed.version}).</Text
                        >
                      {/if}
                      <span class="preview-actions">
                        <Button
                          testid="discover-add-install"
                          size="small"
                          capsule
                          hit
                          disabled={!!preview.installed || !!busy[preview.url]}
                          onclick={() => (confirming = true)}>Install…</Button
                        >
                        <Button
                          testid="discover-add-cancel"
                          variant="plain"
                          size="small"
                          hit
                          onclick={() => (preview = null)}>Cancel</Button
                        >
                      </span>
                    </div>
                  </div>
                {/if}
              </form>
            </section>
          {/if}

          {#if showAgents}
            <hr />

            <section data-testid="discover-agents">
              <header class="section-head">
                <Text style="title3" weight="bold" as="h2">Agents on this Host</Text>
              </header>
              <div class="listings">
                {#each roster as agent (agent.id)}
                  <AgentListing
                    testid="discover-agent-{agent.id}"
                    name={agent.name}
                    blurb={agent.role || 'Chat with it in the Agents app.'}
                    icon={iconOf(agent.avatar ?? 'sparkles')}
                    background={tintBackground('purple')}
                    action={agentsRunning ? 'open' : 'none'}
                    onget={openAgents}
                  />
                {:else}
                  <Text style="subheadline" color="secondary">
                    {query
                      ? `No agents match “${query}”.`
                      : 'No agents on this host yet. Add one to vos.toml.'}
                  </Text>
                {/each}
              </div>
              {#if host.agentsError}
                <p class="store-error" role="alert" data-testid="discover-agents-error">
                  {host.agentsError}
                </p>
              {/if}
            </section>
          {/if}

          <hr />

          <section>
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
        </div>
      {/snippet}
    </ScreenTemplate>
  {/snippet}
</SplitViewTemplate>

{#if preview}
  <Alert
    testid="discover-install-confirm"
    bind:open={confirming}
    title="Install {preview.name}?"
    message="{preview.name} will run on this host, not in your browser, and the tools it provides will be available to your agents. {accessNote} Only install apps you trust."
    actions={[
      { label: 'Cancel', style: 'cancel' },
      { label: 'Install', preferred: true, onclick: () => void confirmInstall() }
    ]}
  />
{/if}

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
  .hero-actions {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 12px;
  }
  .hero-cta {
    color: #4b2bd6;
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
  .add {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 22px;
    padding: 16px;
    border-radius: var(--radius-lg);
    background: var(--bg-grouped-elevated, var(--bg-elevated));
    border: var(--hairline) solid var(--separator);
  }
  .add :global(h3),
  .add :global(p) {
    margin: 0;
  }
  .add-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .url-box {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    min-height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-md);
    background: var(--fill-tertiary, var(--bg-grouped));
    border: var(--hairline) solid var(--separator);
  }
  .url-box:focus-within {
    outline: 3px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .preview {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding-top: 6px;
  }
  .preview-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .preview-actions {
    display: flex;
    gap: 4px;
    margin-top: 6px;
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
    grid-template-columns: repeat(3, 1fr);
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
    .hero-actions {
      flex-wrap: wrap;
    }
    .info {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
