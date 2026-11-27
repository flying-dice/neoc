<!-- @molecule StepGroup — a run of an agent's steps between its words, as one quiet row that counts what they did (`Ran 3 commands · Read 2 files`); opened in place, a line for each step, which opens in turn to its details -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import ToolStep from '../atoms/ToolStep.svelte'
  import { summaryOf } from '../lib/items'

  interface Props {
    /** The run: two or more `tool` and `reasoning` items, in order. */
    steps: Item[]
    /** Open a web address a tool gave outside the app. */
    onlink: (url: string) => void
  }

  let { steps, onlink }: Props = $props()

  // counted again as the run grows during the turn; the row stays as the person left it, closed or open
  const summary = $derived(summaryOf(steps))
  // the steps are drawn once the row is opened: a closed run is its row alone
  let open = $state(false)
</script>

<details class="step group" bind:open data-testid="agents-steps-{steps[0]?.id}">
  <summary data-testid="agents-steps-{steps[0]?.id}-summary">
    <ui-icon class="kind" name={summary.icon} size={17}></ui-icon>
    <span class="title">{summary.text}</span>
    {#if summary.failed > 0}<span class="bad">· {summary.failed} failed</span>{/if}
    <ui-icon class="more" name="chevron.right" size={13} weight={2.5}></ui-icon>
  </summary>
  {#if open}
    <div class="steps">
      {#each steps as item (item.id)}
        <ToolStep {item} {onlink} />
      {/each}
    </div>
  {/if}
</details>

<style>
  .group {
    align-self: stretch;
    min-width: 0;
    font: var(--type-subheadline);
    color: var(--label-secondary);
  }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 2px 0;
    list-style: none;
    cursor: pointer;
    border-radius: var(--radius-sm);
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary:hover .title {
    color: var(--label);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bad {
    flex: none;
    color: var(--ios-red);
  }
  .more {
    transition: transform var(--duration-fast, 0.15s) var(--ease-ios, ease);
  }
  .group[open] > summary .more {
    transform: rotate(90deg);
  }
  /* the steps, under the row and set in from it, one line each */
  .steps {
    display: flex;
    flex-direction: column;
    margin: 0 0 4px 9px;
    padding-inline-start: 13px;
    border-inline-start: 1px solid var(--separator);
  }
</style>
