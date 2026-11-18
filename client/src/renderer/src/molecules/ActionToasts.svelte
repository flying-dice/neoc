<!--
  @molecule ActionToasts — the toasts of one window that carry an action button ("Undo"). The host keeps the
  app's closure; tapping the button reports it back. Plain toasts are the renderer's own (Notices). Atoms: Button,
  Text.
  Test ids: `testid` on the stack, `${testid}-<id>` on a toast, `${testid}-<id>-action` and `-dismiss` on its buttons.
-->
<script lang="ts">
  import { Button, Icon, Text } from '@agentos/ui/atoms'

  export interface ActionToast {
    id: number
    text: string
    action: { title: string; id: string }
  }

  interface Props {
    testid: string
    toasts: ActionToast[]
    onaction: (toast: ActionToast) => void
    ondismiss: (toast: ActionToast) => void
  }

  let { testid, toasts, onaction, ondismiss }: Props = $props()
</script>

<div class="toasts" data-testid={testid}>
  {#each toasts as toast (toast.id)}
    <div class="toast" role="status" data-testid="{testid}-{toast.id}">
      <span class="text"><Text style="subheadline" color="inherit">{toast.text}</Text></span>
      <Button
        testid="{testid}-{toast.id}-action"
        variant="plain"
        size="small"
        onclick={() => onaction(toast)}>{toast.action.title}</Button
      >
      <Button
        testid="{testid}-{toast.id}-dismiss"
        variant="plain"
        size="small"
        aria-label="Dismiss"
        onclick={() => ondismiss(toast)}
      >
        <Icon name="xmark" size={14} weight={2.6} />
      </Button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: absolute;
    inset: auto 12px 12px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .toasts:empty {
    display: none;
  }
  .toast {
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
  .text {
    flex: 1;
    min-width: 0;
  }
</style>
