<!-- @molecule PlanSteps — the agent's plan for its turn as a quiet checklist: each step ticked when done, marked while it is being done -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import { planOf } from '../lib/items'

  interface Props {
    /** A `plan` item. */
    item: Item
  }

  let { item }: Props = $props()

  const plan = $derived(planOf(item))
  // the glyph says where a step is; a screen reader hears it in words
  const STEP: Record<string, string> = { pending: 'To do', in_progress: 'Doing', completed: 'Done' }
</script>

<ul class="step plan" aria-label="Plan" data-testid="agents-item-plan-{item.id}">
  {#each plan as entry, i (i)}
    <li class={entry.status}>
      {#if entry.status === 'completed'}
        <ui-icon class="mark" name="checkmark" size={15} weight={2.5}></ui-icon>
      {:else}
        <span class="mark ring"></span>
      {/if}
      <span class="hidden">{STEP[entry.status] ?? entry.status} </span>
      <span class="what">{entry.content}</span>
    </li>
  {/each}
</ul>

<style>
  .plan {
    align-self: stretch;
    margin: 0;
    padding: 2px 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  li {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  /* the glyph sits in the icon column of a tool step's line, so the steps and the tools line up */
  .mark {
    flex: none;
    width: 17px;
    align-self: center;
    display: inline-flex;
    justify-content: center;
  }
  .ring::before {
    content: '';
    width: 9px;
    height: 9px;
    box-sizing: border-box;
    border-radius: 50%;
    border: 1.5px solid currentColor;
  }
  /* the step being done: its ring filled with the accent, and its words at full strength */
  .in_progress .ring::before {
    border-color: var(--tint);
    background: var(--tint);
  }
  .in_progress .what {
    color: var(--label);
  }
  .completed .what {
    text-decoration: line-through;
  }
  .what {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
