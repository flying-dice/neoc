<!--
  @organism AgentActivityWidget — medium widget showing live agent conversations.
  Atoms: Card, Text, ActivityIndicator. Molecules: AgentAvatar.
  Test ids: `testid` on the widget, `${testid}-conversation-${id}` per row,
  `${testid}-new` on the "Ask Claude" row shown when there are none.
-->
<script lang="ts">
  import { Card, Text, ActivityIndicator } from '@agentos/ui/atoms'
  import AgentAvatar from '../molecules/AgentAvatar.svelte'
  import { lookOf, type Conversation } from '../lib/agents.svelte'

  interface Props {
    testid: string
    conversations: Conversation[]
    onselect?: (id: string) => void
    onnew?: () => void
  }

  let { testid, conversations, onselect, onnew }: Props = $props()
  const recent = $derived(conversations.slice(0, 3))
  const active = $derived(
    conversations.filter((c) => c.status === 'running' || c.status === 'waiting').length
  )
</script>

<Card {testid} size="medium" surface="material" padding="14px 16px">
  <div class="head">
    <Text style="footnote" weight="semibold" color="secondary">AGENTS</Text>
    <Text style="footnote" color="secondary">{active} active</Text>
  </div>
  <div class="rows">
    {#each recent as c (c.id)}
      <button
        type="button"
        class="row"
        data-testid="{testid}-conversation-{c.id}"
        onclick={() => onselect?.(c.id)}
      >
        <AgentAvatar icon={lookOf(c.agentId).icon} color={lookOf(c.agentId).color} size={30} />
        <span class="meta">
          <Text style="subheadline" weight="semibold" truncate>{c.title}</Text>
        </span>
        {#if c.status === 'running' || c.status === 'starting'}
          <ActivityIndicator />
        {:else if c.status === 'waiting'}
          <Text style="caption1" color="secondary">Needs input</Text>
        {:else if c.status === 'error'}
          <Text style="caption1" color="destructive">Error</Text>
        {/if}
      </button>
    {:else}
      <button type="button" class="row" data-testid="{testid}-new" onclick={onnew}>
        <AgentAvatar icon={lookOf('claude').icon} color={lookOf('claude').color} size={30} />
        <span class="meta">
          <Text style="subheadline" weight="semibold">Ask Claude</Text>
          <Text style="caption1" color="secondary">Agents can use your apps</Text>
        </span>
      </button>
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
    cursor: pointer;
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
