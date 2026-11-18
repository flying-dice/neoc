<!--
  @molecule Notices — the window's toasts: errors from handlers, "your edit replaced a change made elsewhere",
  `view.command toast`. Each is a kit Toast: a glyph (the toast's own `icon`, else one by kind), the text, an action
  button when the host gave one (tapping it reports the `toast` event), and a dismiss button. Errors are `alert`s,
  the rest polite `status` messages. They expire on their own. Atoms: Toast, Icon.
-->
<script lang="ts">
  import { Icon, Toast } from '@agentos/ui/atoms'
  import { icons, type IconName } from '@agentos/ui'
  import type { Notice } from '../core/store'

  interface Props {
    testid: string
    notices: Notice[]
    ondismiss: (id: number) => void
    onaction?: (id: number) => void
    /** "Undo" on a notice that offers it (a write replaced a change made elsewhere). */
    onundo?: (id: number) => void
  }

  let { testid, notices, ondismiss, onaction, onundo }: Props = $props()

  const glyph = (n: Notice): IconName =>
    n.icon && n.icon in icons
      ? (n.icon as IconName)
      : n.kind === 'error' || n.kind === 'conflict'
        ? 'exclamationmark.triangle'
        : n.kind === 'success'
          ? 'checkmark'
          : 'info.circle'
</script>

<div class="notices" data-testid={testid}>
  {#each notices as notice (notice.id)}
    <Toast
      testid="{testid}-{notice.id}"
      kind={notice.kind}
      text={notice.text}
      action={notice.undo ? 'Undo' : notice.action?.title}
      onaction={() => (notice.undo ? onundo?.(notice.id) : onaction?.(notice.id))}
      ondismiss={() => ondismiss(notice.id)}
    >
      {#snippet icon()}<Icon name={glyph(notice)} size={18} weight={2.4} />{/snippet}
    </Toast>
  {/each}
</div>

<style>
  .notices {
    position: absolute;
    inset: auto 12px 12px;
    z-index: 1200;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .notices:empty {
    display: none;
  }
</style>
