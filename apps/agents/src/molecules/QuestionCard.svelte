<!-- @molecule QuestionCard — a permission an agent asks for during its turn: what it wants to do, and the answers it offers, one of which the person picks -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import { iconOf, permissionOf, type PermissionOption } from '../lib/items'

  interface Props {
    /** A `permission` item. */
    item: Item
    /** The person picked one of the options the agent offered. */
    onanswer: (option: PermissionOption) => void
  }

  let { item, onanswer }: Props = $props()

  const permission = $derived(permissionOf(item))
  // what each kind of answer means, for the person: the agent's own name for it is the button
  const KINDS: Record<string, string> = {
    allow_once: 'this time',
    allow_always: 'from now on',
    reject_once: 'this time',
    reject_always: 'from now on'
  }
  const allows = (option: PermissionOption): boolean => option.kind.startsWith('allow')
</script>

<div class="question" role="group" aria-label="The agent asks: {item.text}" data-testid="agents-question-{item.id}">
  <p class="lead">The agent asks before it goes on</p>
  <p class="asks">
    <ui-icon name={iconOf(permission.kind)} size={17}></ui-icon>
    <span class="what">{item.text}</span>
  </p>
  {#if !item.done}
    <div class="choices">
      {#each permission.options as option, i (option.option_id)}
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button
          variant={allows(option) ? 'filled' : 'plain'}
          destructive={!allows(option)}
          hit
          label="{option.name} ({KINDS[option.kind] ?? option.kind})"
          onclick={() => onanswer(option)}
          data-testid="agents-question-{item.id}-option-{i}">{option.name}</ui-button
        >
      {/each}
    </div>
  {:else}
    <p class="note" data-testid="agents-question-{item.id}-answer">
      {permission.chosen ? `Answered: ${permission.chosen.name}` : 'Not answered: the turn ended first.'}
    </p>
  {/if}
</div>

<style>
  .question {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-xl);
    border: 1px solid var(--separator);
    background: var(--bg-secondary);
  }
  .asks {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .asks ui-icon {
    color: var(--label-secondary);
  }
  .lead {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .what {
    min-width: 0;
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
  .note {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
