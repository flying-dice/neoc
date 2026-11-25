<!-- @molecule ActivityCard — one thing an agent did in a chat that is not its words: its reasoning, a tool it used, a step, a subagent, an activity or an event of its own -->
<script lang="ts">
  import type { Item } from '@agents/rpc'
  import { failureOf, pretty, toolOutcome } from '../lib/items'

  interface Props {
    /** An item of any kind but `user`, `assistant`, `interrupt` and `error`. */
    item: Item
  }

  let { item }: Props = $props()

  const tool = $derived(item.kind === 'tool' ? toolOutcome(item) : null)
  const failure = $derived(item.kind === 'subagent' ? failureOf(item) : undefined)
  const TITLES: Record<string, string> = { reasoning: 'Reasoning', step: 'Step', subagent: 'Subagent', custom: 'Event', activity: 'Activity', tool: 'Tool' }
  const title = $derived(`${TITLES[item.kind] ?? item.kind}${item.name === '' ? '' : `: ${item.name}`}`)
</script>

<div class="card {item.kind}" data-testid="agents-item-{item.kind}-{item.id}">
  {#if item.kind === 'step' || item.kind === 'subagent'}
    <p class="line">
      <span class="title">{title}</span>
      {#if !item.done}<ui-spinner label="In progress"></ui-spinner>{:else if failure}<span class="bad">failed</span>{:else}<span class="note">done</span>{/if}
    </p>
    {#if item.text !== ''}<p class="note">{item.text}</p>{/if}
    {#if failure}<p class="bad">{failure}</p>{/if}
  {:else}
    <!-- what an agent thought and what a tool was given are there to be read, not in the way: closed until asked for -->
    <details open={item.kind === 'activity' || item.kind === 'custom'}>
      <summary data-testid="agents-item-{item.kind}-{item.id}-summary">
        <span class="title">{title}</span>
        {#if tool?.ours}<span class="note">an app on this machine</span>{/if}
        {#if !item.done}<ui-spinner label="In progress"></ui-spinner>{:else if tool?.failed}<span class="bad">failed</span>{/if}
      </summary>
      {#if item.kind === 'reasoning'}
        <p class="said">{item.text}</p>
      {:else if tool}
        <p class="note">Given</p>
        <pre>{pretty(item.text === '' ? '{}' : item.text)}</pre>
        {#if tool.result !== undefined}
          <p class="note">Answered</p>
          <pre>{pretty(tool.result)}</pre>
        {/if}
      {:else}
        <pre>{pretty(item.data)}</pre>
      {/if}
    </details>
  {/if}
</div>

<style>
  .card {
    align-self: flex-start;
    box-sizing: border-box;
    max-width: min(100%, 72ch);
    min-width: 0;
    padding: 8px 12px;
    border-radius: var(--radius-lg);
    border: 1px solid var(--separator);
    background: var(--bg-secondary);
    font: var(--type-footnote);
  }
  .line,
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
  }
  summary {
    cursor: pointer;
  }
  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .note {
    margin: 0;
    color: var(--label-secondary);
  }
  details .note {
    margin-top: 6px;
  }
  .bad {
    margin: 0;
    color: var(--ios-red);
  }
  .said {
    margin: 6px 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--label-secondary);
  }
  pre {
    margin: 4px 0 0;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: var(--type-caption, 12px ui-monospace, monospace);
    font-family: ui-monospace, monospace;
  }
</style>
