<!-- @molecule Composer — the pill a message is typed in, with the round button that sends it, or stops the answer that is coming -->
<script lang="ts">
  interface Props {
    /** What is typed so far. */
    draft: string
    /** The agent is answering: the button stops it, and nothing can be sent. */
    busy: boolean
    /** The agent asked something: it is answered above, and nothing else can be sent until it is. */
    waiting?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    /** The app's word, which the test ids start with: `agents` gives `agents-composer-input`. */
    testid: string
  }

  let { draft, busy, waiting = false, ondraft, onsend, onstop, testid }: Props = $props()
</script>

<form
  class="composer"
  onsubmit={(e) => {
    e.preventDefault()
    onsend()
  }}
>
  <div class="pill">
    <ui-text-field
      variant="bare"
      label="Message"
      placeholder={waiting ? 'Answer the agent above first' : 'Message'}
      disabled={waiting}
      enterkeyhint="send"
      autocomplete="off"
      hit
      value={draft}
      oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
      data-testid="{testid}-composer-input"
    ></ui-text-field>
    {#if busy}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="gray" square shape="circle" hit label="Stop the answer" onclick={onstop} data-testid="{testid}-composer-stop">
        <ui-icon slot="icon" name="stop" size={14}></ui-icon>
      </ui-button>
    {:else}
      <ui-button type="submit" square shape="circle" hit label="Send" disabled={waiting || draft.trim() === ''} data-testid="{testid}-composer-send">
        <ui-icon slot="icon" name="arrow.up" size={18} weight={2.5}></ui-icon>
      </ui-button>
    {/if}
  </div>
</form>

<style>
  .composer {
    padding: 6px 12px 12px;
  }
  /* one pill holds the field and the button, as a chat app's composer does */
  .pill {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 0 0 16px;
    border: 1px solid var(--separator);
    border-radius: var(--radius-pill);
    background: var(--bg);
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  /* the field has no box of its own in the pill: its focus ring hugs the text */
  ui-text-field::part(input) {
    outline-offset: 4px;
  }
</style>
