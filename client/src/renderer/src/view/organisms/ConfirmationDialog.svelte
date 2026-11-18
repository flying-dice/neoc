<!--
  @organism ConfirmationDialog — a choice among actions: a popover anchored to `anchor` (default the node it was
  written inside) on a regular window, an action sheet at the bottom on a compact one. Actions are Buttons in the
  `actions` slot (`role` cancel, destructive, primary). Any action dismisses; Escape dismisses (and on the action
  sheet runs the cancel action). A portal node; the window content is inert behind the action sheet. Molecules:
  Popover, Alert. Atoms: Button, Text.
-->
<script lang="ts">
  import { Button, Text } from '@agentos/ui/atoms'
  import { Alert, Popover } from '@agentos/ui/molecules'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { alertActions } from '../kit/actions'
  import { optStr } from '../kit/support'
  import { presented } from '../kit/presented.svelte'
  import { anchorElement, modalWhile, portalToWindow } from '../overlay'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  let origin: HTMLElement | null = $state(null)
  const present = presented({ ctx, node: () => node, path: () => path, scope: () => scope, host: () => origin })
  const common = $derived(commonOf(ctx, node, path, scope))
  const compact = $derived(ctx.view.store.env.sizeClass === 'compact')
  const actions = $derived(alertActions(ctx.view, node, path, scope, false))
  const anchor = $derived(present.open && !compact ? anchorElement(ctx, optStr(p('anchor')), origin) : null)
  const title = $derived(str(p('title')))
  const message = $derived(str(p('message')))

  function run(i: number): void {
    actions[i]?.onclick?.({})
    present.dismiss()
  }
</script>

<div class="portal" {@attach portalToWindow(ctx.app, (o) => (origin = o))}>
  {#if compact}
    {#if present.open}<span hidden {@attach modalWhile(ctx.app)}></span>{/if}
    <Alert
      testid={common.testid}
      variant="actionSheet"
      open={present.open}
      {title}
      message={message || undefined}
      actions={actions.length ? actions : [{ label: 'Cancel', style: 'cancel' }]}
      onclose={() => present.dismiss()}
    />
  {:else}
    <Popover
      testid={common.testid}
      open={present.open}
      {anchor}
      arrow={anchor !== null}
      label={title}
      onclose={() => present.dismiss()}
    >
      <div class="dialog">
        <div class="head">
          <Text style="headline">{title}</Text>
          {#if message}<Text style="footnote" color="secondary">{message}</Text>{/if}
        </div>
        <div class="actions">
          {#each actions as action, i (action.key ?? i)}
            <Button
              testid="{common.testid}-action-{action.key ?? i}"
              variant="plain"
              block
              destructive={action.style === 'destructive'}
              disabled={action.disabled}
              onclick={() => run(i)}
            >
              <span class:bold={action.style === 'cancel' || action.preferred}>{action.label}</span>
            </Button>
          {/each}
        </div>
      </div>
    </Popover>
  {/if}
</div>

<style>
  .portal {
    display: contents;
  }
  .dialog {
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-width: 300px;
  }
  .head {
    display: grid;
    gap: 2px;
    padding: 14px 16px 10px;
    text-align: center;
  }
  .actions {
    display: flex;
    flex-direction: column;
    border-top: var(--hairline) solid var(--separator);
  }
  .actions :global(.ios-button) {
    min-height: 44px;
    font-weight: 400;
    border-radius: 0;
  }
  .actions :global(.ios-button + .ios-button) {
    border-top: var(--hairline) solid var(--separator);
  }
  .bold {
    font-weight: 600;
  }
</style>
