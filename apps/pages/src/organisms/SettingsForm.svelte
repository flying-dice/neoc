<!-- @organism SettingsForm — how the sites are served: the port the ones with hostnames share; and under it which Docker the server runs in, with the field that chooses another -->
<script lang="ts">
  import type { Engine, Server } from '@pages/rpc'
  import { ConnectionForm } from '@vos/docker'

  interface Props {
    /** The shared port as it is typed. */
    port: string
    /** Something is typed and not saved. */
    changed?: boolean
    /** A change is in flight: the server may be being made again. */
    working?: boolean
    engine: Engine
    server: Server
    /** The address typed for another Docker. */
    hostDraft: string
    onport: (typed: string) => void
    onsave: () => void
    onreset: () => void
    onhostdraft: (address: string) => void
    onconnect: () => void
  }

  let { port, changed = false, working = false, engine, server, hostDraft, onport, onsave, onreset, onhostdraft, onconnect }: Props = $props()

  const where = $derived(engine.host === '' ? 'This machine' : engine.host)
</script>

<section class="settings" aria-label="Settings">
  <form
    class="group"
    onsubmit={(e) => {
      e.preventDefault()
      if (changed && !working) onsave()
    }}
  >
    <label class="title" for="pages-port">Port for hostnames</label>
    <ui-text-field
      id="pages-port"
      inputmode="numeric"
      autocomplete="off"
      maxlength={5}
      hit
      value={port}
      oninput={(e) => onport((e.currentTarget as unknown as { value: string }).value)}
      data-testid="pages-port"
    ></ui-text-field>
    <p class="hint">
      Every site with a hostname answers on this port, told apart by the name it is asked for. Browsers ask on 80: with another, the port is
      part of the address.
    </p>
    <div class="buttons">
      {#if changed}
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button variant="plain" hit disabled={working} onclick={onreset} data-testid="pages-settings-reset">Revert</ui-button>
      {/if}
      <ui-button type="submit" hit disabled={!changed || working} data-testid="pages-settings-save">Save</ui-button>
    </div>
  </form>

  <div class="group docker">
    <h2>Docker</h2>
    <p class="about" data-testid="pages-engine">
      <span class="name">{engine.reachable ? `Docker ${engine.version}` : 'Docker'}</span>
      <span class="where">{where}</span>
    </p>
    <p class="hint">
      The server is nginx, in a container this app makes and keeps (<code data-testid="pages-image">{server.image}</code>). It reads the folders
      of the sites and changes nothing in them. It is removed when no site is on, and made again when something here changes.
    </p>
    <ConnectionForm draft={hostDraft} host={engine.host} ondraft={onhostdraft} {onconnect} testid="pages" />
  </div>
</section>

<style>
  .settings {
    box-sizing: border-box;
    height: 100%;
    overflow-y: auto;
    padding: 4px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .title {
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .hint {
    margin: 0 0 6px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
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
