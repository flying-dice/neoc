<!--
  @page DiscoverPage — the landing window: a store-style "Discover" app in an
  iPadOS split view, introducing AgentOS and its featured agents.
-->
<script lang="ts">
  import type { IconName } from '@agentos/ui'
  import { Button, Icon, IconTile, Text } from '@agentos/ui/atoms'
  import { IconButton, ListRow, SearchField } from '@agentos/ui/molecules'
  import { ListSection, NavigationBar } from '@agentos/ui/organisms'
  import { ScreenTemplate, SplitViewTemplate } from '@agentos/ui/templates'
  import AgentListing from '../molecules/AgentListing.svelte'
  import CapabilityCard from '../molecules/CapabilityCard.svelte'
  import { windowManager as wm } from '../lib/windowManager.svelte'
  import { appStore } from '../lib/appStore.svelte'
  import type { KnownApp } from '@agentos/sdk/host/client'

  const knownApps = $derived(
    appStore.known.filter((a) => a.manifest.name.toLowerCase().includes(query.toLowerCase()))
  )

  async function primary(app: KnownApp): Promise<void> {
    const state = appStore.stateOf(app)
    if (state === 'open') return wm.open(app.manifest.id)
    if (await appStore.install(app)) wm.open(app.manifest.id)
  }

  type Section =
    'discover' | 'agents' | 'create' | 'work' | 'play' | 'develop' | 'categories' | 'updates'

  const sections: { id: Section; label: string; icon: IconName; badge?: number }[] = [
    { id: 'discover', label: 'Discover', icon: 'star' },
    { id: 'agents', label: 'Agents', icon: 'sparkles' },
    { id: 'create', label: 'Create', icon: 'paintbrush' },
    { id: 'work', label: 'Work', icon: 'briefcase' },
    { id: 'play', label: 'Play', icon: 'play' },
    { id: 'develop', label: 'Develop', icon: 'hammer' },
    { id: 'categories', label: 'Categories', icon: 'square.grid' },
    { id: 'updates', label: 'Updates', icon: 'arrow.down.circle', badge: 2 }
  ]

  let section = $state<Section>('discover')
  let query = $state('')
  let installed = $state<Record<string, boolean>>({ research: true })

  const featured = [
    {
      id: 'research',
      name: 'Research',
      blurb: 'Reads the web and papers, then writes the brief.',
      icon: 'globe',
      background: 'linear-gradient(180deg,#5ac8fa,#007aff)'
    },
    {
      id: 'coder',
      name: 'Coder',
      blurb: 'Plans, writes and reviews code in your repos.',
      icon: 'terminal',
      background: 'linear-gradient(180deg,#7d7aff,#4b45d6)'
    },
    {
      id: 'inbox',
      name: 'Inbox Zero',
      blurb: 'Triages mail and drafts replies for approval.',
      icon: 'message',
      background: 'linear-gradient(180deg,#5fe08a,#20b04b)'
    },
    {
      id: 'ops',
      name: 'Ops Watch',
      blurb: 'Monitors services and pages you when it matters.',
      icon: 'cpu',
      background: 'linear-gradient(180deg,#ffb340,#ff7a00)'
    },
    {
      id: 'designer',
      name: 'Designer',
      blurb: 'Turns rough ideas into polished mockups.',
      icon: 'paintbrush',
      background: 'linear-gradient(180deg,#ff6b9a,#e6245e)'
    },
    {
      id: 'analyst',
      name: 'Analyst',
      blurb: 'Pulls numbers together and charts the trend.',
      icon: 'chart.bar',
      background: 'linear-gradient(180deg,#5ee0d6,#0fa3a3)'
    }
  ] satisfies { id: string; name: string; blurb: string; icon: IconName; background: string }[]

  const visible = $derived(
    featured.filter((f) => `${f.name} ${f.blurb}`.toLowerCase().includes(query.toLowerCase()))
  )

  const information: [string, string][] = [
    ['Developer', 'AgentOS'],
    ['Version', '0.1.0'],
    ['Category', 'Productivity'],
    ['Compatibility', 'macOS, Windows, Linux'],
    ['Agents Running', '3 of 4'],
    ['Languages', 'English and 7 more']
  ]
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
          value={s.badge ? String(s.badge) : undefined}
          selected={section === s.id}
          onclick={() => (section = s.id)}
        />
      {/each}
    </ListSection>
    <ListSection sidebar>
      <ListRow testid="discover-account" title="AgentOS User" subtitle="4 agents">
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
        <NavigationBar testid="discover-nav" title="AgentOS">
          {#snippet trailing()}
            <Button testid="discover-open" size="small" capsule onclick={() => wm.open('agents')}
              >OPEN</Button
            >
            <IconButton testid="discover-share" icon="square.and.arrow.up" label="Share" />
          {/snippet}
        </NavigationBar>
      {/snippet}

      {#snippet content()}
        <div class="page">
          <section class="hero">
            <div class="hero-copy">
              <Text style="caption1" weight="semibold" color="inherit">WELCOME TO AGENTOS</Text>
              <h1>Your agents, on one desktop.</h1>
              <p>
                Run research, code, inbox and ops agents side by side. Watch them work, step in when
                they need you, and keep everything on your machine.
              </p>
              <div class="hero-actions">
                <Button
                  testid="discover-hero-open-agents"
                  size="large"
                  capsule
                  tint="#fff"
                  onclick={() => wm.open('agents')}
                >
                  <span class="hero-cta">Open Agents</span>
                </Button>
                <Button
                  testid="discover-hero-ui-kit"
                  variant="plain"
                  tint="#fff"
                  onclick={() => wm.open('gallery')}>Explore the UI Kit ›</Button
                >
              </div>
            </div>
            <div class="hero-art" aria-hidden="true">
              {#each featured.slice(0, 4) as f, i (f.id)}
                <span class="float f{i}">
                  <IconTile size={84 - i * 8} background={f.background} shadow>
                    <Icon name={f.icon} size={44 - i * 4} weight={1.7} color="#fff" />
                  </IconTile>
                </span>
              {/each}
            </div>
          </section>

          <section data-testid="discover-apps">
            <header class="section-head">
              <Text style="title3" weight="bold" as="h2">Apps</Text>
            </header>
            {#if appStore.error}
              <p class="store-error" role="alert" data-testid="discover-apps-error">
                {appStore.error}
              </p>
            {/if}
            <div class="listings">
              {#each knownApps as app (app.manifest.id)}
                <AgentListing
                  testid="discover-app-{app.manifest.id}"
                  name={app.manifest.name}
                  blurb="Version {app.installedVersion &&
                  app.installedVersion !== app.manifest.version
                    ? `${app.installedVersion} → ${app.manifest.version}`
                    : app.manifest.version}"
                  icon={app.manifest.icon}
                  background={app.manifest.background}
                  action={appStore.stateOf(app)}
                  busy={!!appStore.busy[app.manifest.id]}
                  onget={() => void primary(app)}
                  onremove={app.installedVersion ? () => void appStore.uninstall(app) : undefined}
                />
              {:else}
                <Text style="subheadline" color="secondary">
                  {query
                    ? `No apps match “${query}”.`
                    : 'No apps are available to install. Run `bun run build:apps`.'}
                </Text>
              {/each}
            </div>
          </section>

          <hr />

          <section>
            <header class="section-head">
              <Text style="title3" weight="bold" as="h2">Featured Agents</Text>
              <Button testid="discover-featured-see-all" variant="plain">See All</Button>
            </header>
            <div class="listings">
              {#each visible as f (f.id)}
                <AgentListing
                  testid="discover-listing-{f.id}"
                  name={f.name}
                  blurb={f.blurb}
                  icon={f.icon}
                  background={f.background}
                  installed={installed[f.id]}
                  onget={() => (installed[f.id] ? wm.open('agents') : (installed[f.id] = true))}
                />
              {:else}
                <Text style="subheadline" color="secondary">No agents match “{query}”.</Text>
              {/each}
            </div>
          </section>

          <hr />

          <section>
            <header class="section-head">
              <Text style="title3" weight="bold" as="h2">Agent Privacy</Text>
              <Button
                testid="discover-privacy-details"
                variant="plain"
                onclick={() => wm.open('settings')}>See Details</Button
              >
            </header>
            <Text style="footnote" color="secondary" as="p">
              Every agent declares what it can do before it runs. You approve capabilities once and
              can revoke them any time in Settings.
            </Text>
            <div class="capabilities">
              <CapabilityCard
                title="Can Act for You"
                description="With approval, these agents can take actions on your behalf:"
                icon="hand.raised"
                items={[
                  { icon: 'globe', label: 'Browse' },
                  { icon: 'terminal', label: 'Run Shell' },
                  { icon: 'message', label: 'Send Drafts' }
                ]}
              />
              <CapabilityCard
                title="Can Read"
                description="Agents may read the following to complete a task:"
                icon="eye"
                items={[
                  { icon: 'folder', label: 'Files' },
                  { icon: 'message', label: 'Mail' },
                  { icon: 'clock', label: 'History' },
                  { icon: 'doc', label: 'Documents' }
                ]}
              />
              <CapabilityCard
                title="Never Leaves Device"
                description="This data is processed locally and never shared:"
                icon="eye.slash"
                items={[
                  { icon: 'lock', label: 'Credentials' },
                  { icon: 'person', label: 'Contacts' }
                ]}
              />
            </div>
          </section>

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
  .capabilities {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 16px;
    margin-top: 16px;
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
