<!--
  @organism Alert — a modal alert with a title, a message, optional prompt `fields` and `actions` (Buttons in the
  `actions` slot; `role` cancel, destructive or primary). A portal node with the window content inert behind it.
  Focus lands on the preferred action (or the first field) and stays in the alert; Escape runs the cancel action;
  any action dismisses (binding false, then the action's own tap, then `dismiss`). Entered field values travel in
  the action's tap payload as `fields`. Molecules: Alert.
-->
<script lang="ts">
  import { Alert } from '@vos/ui/molecules'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { alertActions, alertFields } from '../kit/actions'
  import { presented } from '../kit/presented.svelte'
  import { modalWhile, portalToWindow } from '../overlay'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  let origin: HTMLElement | null = $state(null)
  const present = presented({
    ctx,
    node: () => node,
    path: () => path,
    scope: () => scope,
    host: () => origin
  })
  const common = $derived(commonOf(ctx, node, path, scope))
  const fields = $derived(alertFields(p('fields')))
  const actions = $derived(alertActions(ctx.view, node, path, scope, fields.length > 0))
</script>

<div class="portal" {@attach portalToWindow(ctx.app, (o) => (origin = o))}>
  {#if present.open}<span hidden {@attach modalWhile(ctx.app)}></span>{/if}
  <Alert
    testid={common.testid}
    open={present.open}
    title={str(p('title'))}
    message={str(p('message')) || undefined}
    {fields}
    actions={actions.length ? actions : undefined}
    onclose={() => present.dismiss()}
  />
</div>

<style>
  .portal {
    display: contents;
  }
</style>
