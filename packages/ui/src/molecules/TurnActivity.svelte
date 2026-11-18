<!--
  @molecule TurnActivity — the single feedback line for an agent turn: a live
  spinner while it works, then counts of reasoning steps and tool calls.
  Atoms: Icon, ActivityIndicator, Text.
-->
<script lang="ts">
  import ActivityIndicator from '../atoms/ActivityIndicator.svelte'
  import Icon from '../atoms/Icon.svelte'
  import Text from '../atoms/Text.svelte'

  interface Props {
    reasoning: number
    tools: number
    failed?: number
    /** The turn is still in progress. */
    live?: boolean
    /** Override the live label (e.g. "Starting Claude…"). */
    status?: string
  }

  let { reasoning, tools, failed = 0, live = false, status }: Props = $props()

  const plural = (n: number, word: string): string => `${n} ${word}${n === 1 ? '' : 's'}`
  const counts = $derived(
    [
      reasoning && plural(reasoning, 'reasoning step'),
      tools && plural(tools, 'tool call'),
      failed && `${failed} failed`
    ]
      .filter(Boolean)
      .join(' · ')
  )
  const label = $derived(live ? [status ?? 'Working', counts].filter(Boolean).join(' · ') : counts)
</script>

{#if live || counts}
  <div class="activity" class:failed={failed > 0} role="status">
    <span class="glyph">
      {#if live}
        <ActivityIndicator label="Working" />
      {:else if failed}
        <Icon name="exclamationmark.triangle" size={13} weight={2.2} />
      {:else}
        <Icon name="checkmark" size={13} weight={2.6} />
      {/if}
    </span>
    <Text style="caption1" color="secondary" truncate>{label}</Text>
  </div>
{/if}

<style>
  .activity {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: min(70%, 520px);
    margin: 2px 16px;
    padding: 3px 10px 3px 6px;
    border-radius: 999px;
    background: var(--fill-quaternary);
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    flex: none;
    color: var(--ios-green);
  }
  .failed .glyph {
    color: var(--ios-orange);
  }
</style>
