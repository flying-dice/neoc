<!--
  @molecule ConnectionBanner — shown over a dimmed window while the connection is down: what is happening (the
  client keeps trying with backoff), what still works (typing; drafts are kept and sent on reconnect) and a Retry.
  A polite `status` live region. Atoms: ActivityIndicator, Button, Text.
-->
<script lang="ts">
  import { ActivityIndicator, Button, Text } from '@vos/ui/atoms'

  interface Props {
    testid: string
    /** How many edits are waiting to be sent. */
    queued: number
    onretry?: () => void
  }

  let { testid, queued, onretry }: Props = $props()
</script>

<div class="banner" role="status" data-testid={testid}>
  <ActivityIndicator label="Reconnecting" />
  <span class="text">
    <Text style="subheadline" weight="semibold">Reconnecting…</Text>
    <Text style="footnote" color="secondary">
      {queued > 0
        ? `${queued} ${queued === 1 ? 'edit is' : 'edits are'} saved here and will be sent when you are back.`
        : 'You can keep typing; edits are sent when you are back.'}
    </Text>
  </span>
  {#if onretry}<Button testid="{testid}-retry" variant="gray" size="small" onclick={onretry}
      >Retry</Button
    >{/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    background: var(--bg-grouped-elevated-secondary, var(--bg-elevated));
    border-bottom: var(--hairline) solid var(--separator);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
