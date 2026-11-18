<!--
  @molecule Notices — the window's toasts: errors from handlers, "your edit replaced a change made elsewhere",
  `view.command toast`. Errors are `alert`s, the rest polite `status` messages. Each can be dismissed; they also
  expire on their own. Atoms: Button, Icon, Text.
-->
<script lang="ts">
  import { Button, Icon, Text } from '@agentos/ui/atoms'
  import type { Notice } from '../core/store'

  interface Props {
    testid: string
    notices: Notice[]
    ondismiss: (id: number) => void
  }

  let { testid, notices, ondismiss }: Props = $props()
</script>

<div class="notices" data-testid={testid}>
  {#each notices as notice (notice.id)}
    <div
      class="notice {notice.kind}"
      role={notice.kind === 'error' ? 'alert' : 'status'}
      data-testid="{testid}-{notice.id}"
    >
      <Icon
        name={notice.kind === 'error' || notice.kind === 'conflict'
          ? 'exclamationmark.triangle'
          : 'checkmark'}
        size={16}
        weight={2.4}
      />
      <span class="text"><Text style="subheadline" color="inherit">{notice.text}</Text></span>
      <Button
        testid="{testid}-{notice.id}-dismiss"
        variant="plain"
        size="small"
        aria-label="Dismiss"
        onclick={() => ondismiss(notice.id)}
      >
        <Icon name="xmark" size={14} weight={2.6} />
      </Button>
    </div>
  {/each}
</div>

<style>
  .notices {
    position: absolute;
    inset: auto 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .notices:empty {
    display: none;
  }
  .notice {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 14px;
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    border: var(--hairline) solid var(--separator);
  }
  .error {
    color: var(--ios-red);
  }
  .conflict {
    color: var(--ios-orange);
  }
  .text {
    flex: 1;
    min-width: 0;
    color: var(--label);
  }
</style>
