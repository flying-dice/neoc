<!-- @organism SettingsForm — how the server answers, as a form of iPadOS is: a row each for its port, who it asks for the names it does not know and whether it logs each question, what is wrong with them in the footer under the rows, and a Save that cannot be pressed until there is something right to save; under it which Docker the server runs in, with the field that chooses another -->
<script lang="ts">
  import type { Engine, Server } from 'app:rpc'
  import { ConnectionForm } from '@vos/docker'
  import type { Problems, SettingsDraft } from '../lib/dns.svelte'

  interface Props {
    /** The settings as they are typed. */
    draft: SettingsDraft
    /** What is wrong with each field as it is typed. */
    problems: Problems<'port' | 'upstreams'>
    /** The backend's word on the settings that were sent and not saved. */
    refused?: string | null
    /** Something is typed and not saved. */
    changed?: boolean
    /** A change is in flight: the server is being made again. */
    working?: boolean
    engine: Engine
    server: Server
    /** The address typed for another Docker. */
    hostDraft: string
    ondraft: (change: Partial<SettingsDraft>) => void
    onsave: () => void
    onreset: () => void
    onhostdraft: (address: string) => void
    onconnect: () => void
  }

  let { draft, problems, refused = null, changed = false, working = false, engine, server, hostDraft, ondraft, onsave, onreset, onhostdraft, onconnect }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const NOTE = 'Other machines ask on port 53. The upstream servers are asked for every name that is not one of yours: their addresses, apart by commas.'

  // The port is a few digits: what is wrong with it is said as it is typed. An address is only wrong once it is
  // finished, so the servers say it once the person has left the field.
  let left = $state(false)
  const upstreamsProblem = $derived(left ? problems.upstreams : null)
  // one footer for the rows: the first thing that is wrong, top to bottom, then what the backend refused
  const problem = $derived(problems.port ?? upstreamsProblem ?? refused)
  // there is something to save, and nothing wrong with it: only then can Save be pressed
  const ready = $derived(changed && !working && problems.port === null && problems.upstreams === null)
  const where = $derived(engine.host === '' ? 'This machine' : engine.host)
</script>

<section class="settings" aria-label="Settings">
  <form
    class="form"
    novalidate
    onsubmit={(e) => {
      e.preventDefault()
      // Return in a field: what is wrong is said now
      left = true
      if (ready) onsave()
    }}
  >
    <ui-list variant="inset" label="How the server answers">
      <ui-section footer={problem ?? NOTE} footer-tone={problem ? 'error' : 'note'} data-testid="dns-settings-form">
        <ui-row>
          <span class="key">Port</span>
          <ui-text-field
            variant="bare"
            label="Port"
            description={problems.port ?? undefined}
            inputmode="numeric"
            autocomplete="off"
            maxlength={5}
            invalid={problems.port !== null}
            value={draft.port}
            oninput={(e) => ondraft({ port: valueOf(e) })}
            data-testid="dns-port"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Upstream</span>
          <ui-text-field
            variant="bare"
            label="Upstream servers"
            description={(upstreamsProblem ?? refused) || undefined}
            placeholder="None: only your names are answered"
            autocomplete="off"
            autocapitalize="off"
            maxlength={400}
            invalid={upstreamsProblem !== null || (problems.port === null && refused !== null)}
            value={draft.upstreams}
            oninput={(e) => ondraft({ upstreams: valueOf(e) })}
            onfocusout={() => (left = true)}
            data-testid="dns-upstreams"
          ></ui-text-field>
        </ui-row>
        <ui-row heading="Log every question">
          <ui-toggle
            slot="trailing"
            label="Log every question"
            checked={draft.logQueries}
            onchange={(e) => ondraft({ logQueries: e.detail.checked })}
            data-testid="dns-log-queries"
          ></ui-toggle>
        </ui-row>
      </ui-section>
    </ui-list>
    <div class="buttons">
      {#if changed}
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button
          variant="plain"
          hit
          disabled={working}
          onclick={() => {
            left = false
            onreset()
          }}
          data-testid="dns-settings-reset">Revert</ui-button
        >
      {/if}
      <ui-button type="submit" hit disabled={!ready} data-testid="dns-settings-save">Save</ui-button>
    </div>
  </form>

  <div class="group docker">
    <h2>Docker</h2>
    <p class="about" data-testid="dns-engine">
      <span class="name">{engine.reachable ? `Docker ${engine.version}` : 'Docker'}</span>
      <span class="where">{where}</span>
    </p>
    <p class="hint">
      The server is dnsmasq, in a container this app makes and keeps (<code data-testid="dns-image">{server.image}</code>). It is removed when the
      server is turned off, and made again when something here changes.
    </p>
    <ConnectionForm draft={hostDraft} host={engine.host} ondraft={onhostdraft} {onconnect} testid="dns" />
  </div>
</section>

<style>
  .settings {
    box-sizing: border-box;
    height: 100%;
    overflow-y: auto;
    padding: 4px 0 16px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* the rows line up with the rest of the window, and the buttons sit close under their footer */
    --list-inset: 16px;
  }
  ui-section {
    margin-bottom: 0;
  }
  .key {
    flex: none;
    min-width: 84px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-inline: 16px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-inline: 16px;
  }
  .hint {
    margin: 0 0 6px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  h2 {
    margin: 0;
    font: var(--type-headline);
  }
  .about {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 8px;
    font: var(--type-subheadline);
  }
  .where {
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  code {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
</style>
