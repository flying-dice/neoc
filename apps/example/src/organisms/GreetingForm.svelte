<!-- @organism GreetingForm — the round trip in one form: a name typed, sent to the backend with Greet, and its answer under the row; what the backend refused is said in the footer -->
<script lang="ts">
  interface Props {
    /** The name as it is typed. */
    name: string
    /** The backend's last answer. */
    greeting?: string | null
    /** The backend's word on the name that was sent. */
    refusal?: string | null
    /** Greet can be pressed: there is a name, and nothing in flight. */
    ready?: boolean
    /** A greeting is in flight. */
    sending?: boolean
    /** Show the way back to the sidebar (a window too narrow for it). */
    back?: boolean
    ontype: (name: string) => void
    ongreet: () => void
    onback?: () => void
  }

  let { name, greeting = null, refusal = null, ready = false, sending = false, back = false, ontype, ongreet, onback }: Props = $props()

  const NOTE = 'The name goes to the backend (server/main.luau), which answers with a sentence.'
  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
</script>

<section class="detail" aria-label="Say Hello">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Example" onclick={onback} data-testid="example-hello-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Example
      </ui-button>
    {/if}
    <h1>Say Hello</h1>
  </header>
  <div class="scroll">
    <form
      class="form"
      novalidate
      onsubmit={(e) => {
        e.preventDefault()
        if (ready) ongreet()
      }}
    >
      <ui-list variant="inset" label="Say Hello">
        <ui-section footer={refusal ?? NOTE} footer-tone={refusal ? 'error' : 'note'} data-testid="example-hello-form">
          <ui-row>
            <span class="key">Name</span>
            <ui-text-field
              variant="bare"
              label="Name"
              description={refusal ?? undefined}
              placeholder="Who to greet"
              autocomplete="off"
              maxlength={100}
              enterkeyhint="send"
              invalid={refusal !== null}
              value={name}
              oninput={(e: Event) => ontype(valueOf(e))}
              data-testid="example-hello-name"
            ></ui-text-field>
          </ui-row>
        </ui-section>
      </ui-list>
      <div class="buttons">
        {#if sending}<ui-spinner label="Waiting for the backend" data-testid="example-hello-busy"></ui-spinner>{/if}
        <ui-button type="submit" hit disabled={!ready} data-testid="example-hello-greet">Greet</ui-button>
      </div>
    </form>
    {#if greeting}
      <ui-list variant="inset" label="Answer">
        <ui-section header="Answer">
          <ui-row heading={greeting} data-testid="example-hello-greeting"></ui-row>
        </ui-section>
      </ui-list>
    {/if}
  </div>
</section>

<style>
  .detail {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped);
  }
  .bar {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 0 16px 8px;
  }
  h1 {
    margin: 0;
    font: var(--type-title2);
    font-weight: 700;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* the buttons sit close under their footer */
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
    align-items: center;
    gap: 8px;
    padding-inline: 16px;
  }
</style>
