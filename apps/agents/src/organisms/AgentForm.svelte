<!-- @organism AgentForm — the form an agent is added or changed in: its name, its AG-UI endpoint and its token, with the way to remove it -->
<script lang="ts">
  import type { AgentForm } from '../lib/agents.svelte'

  interface Props {
    form: AgentForm
    /** Why the backend refused the form, shown under the fields. */
    failure?: string | null
    savable: boolean
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  let { form = $bindable(), failure = null, savable, onsave, oncancel, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const adding = $derived(form.agent === null)
  const hasToken = $derived(form.agent?.token === true)
</script>

<form
  class="form"
  aria-label={adding ? 'Add an agent' : 'Change the agent'}
  data-testid="agents-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit onclick={oncancel} data-testid="agents-form-cancel">Cancel</ui-button>
    <h1>{adding ? 'New agent' : 'Agent'}</h1>
    <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="agents-form-save">{adding ? 'Add' : 'Save'}</ui-button>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="The agent">
    <ui-section
      header="Agent"
      footer={failure ?? 'The endpoint is where the agent takes an AG-UI run: this app posts the conversation to it and reads the events it answers with.'}
      footer-tone={failure ? 'error' : 'note'}
      data-testid="agents-form-agent"
    >
      <ui-row>
        <span class="key">Name</span>
        <ui-text-field
          label="Name"
          placeholder="Name"
          maxlength={80}
          autocomplete="off"
          variant="bare"
          value={form.name}
          oninput={(e: Event) => (form.name = valueOf(e))}
          data-testid="agents-form-name"
        ></ui-text-field>
      </ui-row>
      <ui-row>
        <span class="key">Endpoint</span>
        <ui-text-field
          label="Endpoint"
          placeholder="https://example.com/agent"
          type="url"
          inputmode="url"
          autocapitalize="off"
          autocomplete="off"
          maxlength={2000}
          variant="bare"
          value={form.url}
          oninput={(e: Event) => (form.url = valueOf(e))}
          data-testid="agents-form-url"
        ></ui-text-field>
      </ui-row>
    </ui-section>
    <ui-section
      header="Token"
      footer="Sent to the endpoint as a bearer token with every run. It is kept in this machine's keychain, and is never shown again."
    >
      <ui-row>
        <span class="key">Token</span>
        <ui-text-field
          label="Token"
          placeholder={hasToken ? 'Has a token: write a new one to replace it' : 'None'}
          type="password"
          autocomplete="off"
          maxlength={4000}
          variant="bare"
          disabled={form.forget}
          value={form.token}
          oninput={(e: Event) => (form.token = valueOf(e))}
          data-testid="agents-form-token"
        ></ui-text-field>
      </ui-row>
      {#if hasToken}
        <ui-row heading="Remove the token">
          <ui-toggle
            slot="trailing"
            label="Remove the token"
            checked={form.forget}
            onchange={(e: Event) => (form.forget = (e.currentTarget as unknown as { checked: boolean }).checked)}
            data-testid="agents-form-forget"
          ></ui-toggle>
        </ui-row>
      {/if}
    </ui-section>
    {#if !adding}
      <ui-section footer="Its chats stay, and can be read.">
        <ui-row heading="Remove this agent" action destructive tappable onpress={onremove} data-testid="agents-form-remove"></ui-row>
      </ui-section>
    {/if}
    </ui-list>
  </div>
</form>

<style>
  .form {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 8px 6px;
    border-bottom: 1px solid var(--separator);
  }
  h1 {
    margin: 0;
    font: var(--type-headline);
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 5.5em;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
