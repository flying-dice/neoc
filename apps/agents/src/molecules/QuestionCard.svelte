<!-- @molecule QuestionCard — a question an agent stopped at (an interrupt): what it asks, and the answer or the refusal the person gives -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import { questionStatus } from '../lib/items'

  interface Props {
    /** An `interrupt` item. */
    item: Item
    /** The person answered (what they wrote, which may be nothing) or declined (null). */
    onanswer: (answer: string | null) => void
  }

  let { item, onanswer }: Props = $props()

  let written = $state('')
  const status = $derived(questionStatus(item))
</script>

<form
  class="question"
  data-testid="agents-question-{item.id}"
  onsubmit={(e) => {
    e.preventDefault()
    onanswer(written)
  }}
>
  <p class="asks">{item.text}</p>
  {#if status === 'open'}
    <ui-text-field
      label="Your answer"
      placeholder="Answer (optional)"
      autocomplete="off"
      hit
      value={written}
      oninput={(e: Event) => (written = (e.currentTarget as unknown as { value: string }).value)}
      data-testid="agents-question-{item.id}-input"
    ></ui-text-field>
    <div class="choices">
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" destructive hit onclick={() => onanswer(null)} data-testid="agents-question-{item.id}-decline">Decline</ui-button>
      <ui-button type="submit" variant="filled" hit data-testid="agents-question-{item.id}-answer">Answer</ui-button>
    </div>
  {:else}
    <p class="note">{status === 'resolved' ? 'Answered' : 'Declined'}</p>
  {/if}
</form>

<style>
  .question {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: var(--radius-lg);
    border: 1px solid var(--tint);
    background: var(--bg-secondary);
  }
  .asks {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .choices {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .note {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
