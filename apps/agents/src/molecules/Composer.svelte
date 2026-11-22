<!-- @molecule Composer — the field a message is typed in, with the button that sends it, or stops the answer that is coming -->
<script lang="ts">
  interface Props {
    /** What is typed so far. */
    draft: string
    /** The agent is answering: the button stops it, and nothing can be sent. */
    busy: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
  }

  let { draft, busy, ondraft, onsend, onstop }: Props = $props()
</script>

<form
  class="composer"
  onsubmit={(e) => {
    e.preventDefault()
    onsend()
  }}
>
  <ui-text-field
    label="Message"
    placeholder="Message"
    enterkeyhint="send"
    autocomplete="off"
    hit
    value={draft}
    oninput={(e) => ondraft((e.currentTarget as unknown as { value: string }).value)}
    data-testid="agents-composer-input"
  ></ui-text-field>
  {#if busy}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button square shape="circle" destructive hit label="Stop the answer" onclick={onstop} data-testid="agents-composer-stop">
      <ui-icon slot="icon" name="stop"></ui-icon>
    </ui-button>
  {:else}
    <ui-button type="submit" square shape="circle" hit label="Send" disabled={draft.trim() === ''} data-testid="agents-composer-send">
      <ui-icon slot="icon" name="arrow.up"></ui-icon>
    </ui-button>
  {/if}
</form>

<style>
  .composer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px 12px;
    border-top: 1px solid var(--separator);
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
