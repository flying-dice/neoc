<!--
  @organism AgentActivityWidget — medium widget listing the agents on this host (the host's roster); tapping a row
  opens the Agents app when the host has one (`onopen`; without it the rows are plain text, not buttons). Atoms: Card,
  Text. Molecules: AgentAvatar.
  Test ids: `testid` on the widget, `${testid}-agent-${id}` per row, `${testid}-open` on the row shown when
  there are none.
-->
<script lang="ts">
  import { Card, Text } from '@vos/ui/atoms'
  import AgentAvatar from '../molecules/AgentAvatar.svelte'
  import { iconOf, type AgentSummary } from '../shell/hostApi'

  interface Props {
    testid: string
    agents: AgentSummary[]
    /** Bring the Agents app forward; leave it out when the host has no such app. */
    onopen?: () => void
  }

  let { testid, agents, onopen }: Props = $props()
  const shown = $derived(agents.slice(0, 3))
</script>

{#snippet agentRow(agent: AgentSummary)}
  <AgentAvatar icon={iconOf(agent.avatar ?? 'sparkles')} color="var(--ios-purple)" size={30} />
  <span class="meta">
    <Text style="subheadline" weight="semibold" truncate>{agent.name}</Text>
    <Text style="caption1" color="secondary" truncate>{agent.role || 'Chat with it'}</Text>
  </span>
{/snippet}

{#snippet none()}
  <AgentAvatar icon="sparkles" color="var(--ios-gray)" size={30} />
  <span class="meta">
    <Text style="subheadline" weight="semibold">No agents yet</Text>
    <Text style="caption1" color="secondary">Add one to agentos.toml</Text>
  </span>
{/snippet}

<Card {testid} size="medium" surface="material" padding="14px 16px">
  <div class="head">
    <Text style="footnote" weight="semibold" color="secondary">AGENTS</Text>
    <Text style="footnote" color="secondary">{agents.length} on this host</Text>
  </div>
  <div class="rows">
    {#each shown as agent (agent.id)}
      {#if onopen}
        <button
          type="button"
          class="row"
          data-testid="{testid}-agent-{agent.id}"
          onclick={() => onopen()}
        >
          {@render agentRow(agent)}
        </button>
      {:else}
        <div class="row" data-testid="{testid}-agent-{agent.id}">{@render agentRow(agent)}</div>
      {/if}
    {:else}
      {#if onopen}
        <button type="button" class="row" data-testid="{testid}-open" onclick={() => onopen()}>
          {@render none()}
        </button>
      {:else}
        <div class="row" data-testid="{testid}-open">{@render none()}</div>
      {/if}
    {/each}
  </div>
</Card>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    appearance: none;
    border: 0;
    background: none;
    padding: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    color: inherit;
    cursor: default;
  }
  button.row {
    cursor: pointer;
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
