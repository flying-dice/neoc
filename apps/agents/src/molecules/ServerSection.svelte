<!-- @molecule ServerSection — one MCP server of the agent form: a program the agent starts where it runs (its command, arguments and environment) or a server at a URL (its address and headers), with the way to take it out -->
<script lang="ts">
  type Secret = VosApp.Agents.Secret
  import VariableRow from '../atoms/VariableRow.svelte'
  import { blankVariable, type ServerDraft } from '../lib/form'
  import type { Kind } from '../lib/kinds'

  interface Props {
    /** The server, changed as it is typed in. */
    server: ServerDraft
    /** The Keychain's items a variable or a header can take its value from. */
    secrets: readonly Secret[]
    /** The type of the agent: where a program server runs, and how a URL server reaches this machine. */
    kind: Kind
    /** The section's test id; its fields' follow from it (`-type`, `-name`, `-command`, `-args`, `-url`, `-env-0`, `-header-0`, `-remove`). */
    testid: string
    onremove: () => void
  }

  let { server = $bindable(), secrets, kind, testid, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const where = $derived(kind === 'container' ? 'in the container' : kind === 'program' ? 'on this machine' : 'on its own machine')
  const TYPES = [
    { value: 'stdio', label: 'Program' },
    { value: 'http', label: 'URL' }
  ]
</script>

<ui-section
  header="MCP server"
  footer={server.type === 'stdio'
    ? `A program the agent starts ${where}.`
    : `Given only to an agent that takes MCP servers over HTTP.${kind === 'container' ? ' In the container, this machine is host.docker.internal.' : ''}`}
  data-testid={testid}
>
  <ui-row>
    <ui-segmented
      block
      hit
      label="What the server is"
      options={TYPES}
      value={server.type}
      onchange={(e) => (server.type = e.detail.value as ServerDraft['type'])}
      data-testid="{testid}-type"
    ></ui-segmented>
  </ui-row>
  <ui-row>
    <span class="key">Name</span>
    <ui-text-field
      label="Server name"
      placeholder="files"
      autocapitalize="off"
      autocomplete="off"
      maxlength={100}
      variant="bare"
      value={server.name}
      oninput={(e: Event) => (server.name = valueOf(e))}
      data-testid="{testid}-name"
    ></ui-text-field>
  </ui-row>
  {#if server.type === 'stdio'}
    <ui-row>
      <span class="key">Command</span>
      <ui-text-field
        class="mono"
        label="Server command"
        placeholder="mcp-server-files"
        autocapitalize="off"
        autocomplete="off"
        maxlength={4000}
        variant="bare"
        value={server.command}
        oninput={(e: Event) => (server.command = valueOf(e))}
        data-testid="{testid}-command"
      ></ui-text-field>
    </ui-row>
    <ui-row>
      <span class="key">Arguments</span>
      <ui-text-field
        class="mono"
        label="Server arguments"
        placeholder="--root /srv/files"
        autocapitalize="off"
        autocomplete="off"
        variant="bare"
        value={server.args}
        oninput={(e: Event) => (server.args = valueOf(e))}
        data-testid="{testid}-args"
      ></ui-text-field>
    </ui-row>
    {#each server.env as variable, i (variable.key)}
      <VariableRow
        bind:variable={server.env[i]!}
        kind="Variable"
        {secrets}
        testid="{testid}-env-{i}"
        onremove={() => (server.env = server.env.filter((v) => v.key !== variable.key))}
      />
    {/each}
    <ui-row heading="Add a variable" action tappable onpress={() => server.env.push(blankVariable())} data-testid="{testid}-add-env"></ui-row>
  {:else}
    <ui-row>
      <span class="key">URL</span>
      <ui-text-field
        label="Server URL"
        placeholder={kind === 'container' ? 'http://host.docker.internal:8080/mcp' : 'http://localhost:8080/mcp'}
        type="url"
        inputmode="url"
        autocapitalize="off"
        autocomplete="off"
        maxlength={2000}
        variant="bare"
        value={server.url}
        oninput={(e: Event) => (server.url = valueOf(e))}
        data-testid="{testid}-url"
      ></ui-text-field>
    </ui-row>
    {#each server.headers as header, i (header.key)}
      <VariableRow
        bind:variable={server.headers[i]!}
        kind="Header"
        placeholder="Authorization"
        {secrets}
        testid="{testid}-header-{i}"
        onremove={() => (server.headers = server.headers.filter((h) => h.key !== header.key))}
      />
    {/each}
    <ui-row heading="Add a header" action tappable onpress={() => server.headers.push(blankVariable())} data-testid="{testid}-add-header"></ui-row>
  {/if}
  <ui-row heading="Remove this server" action destructive tappable onpress={onremove} data-testid="{testid}-remove"></ui-row>
</ui-section>

<style>
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 6em;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
