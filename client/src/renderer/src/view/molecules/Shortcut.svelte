<!--
  @molecule Shortcut — an invisible keyboard command: active while the window has focus (and not while typing, for
  keys with no command modifier), listed in the window's command list (`view.shortcuts`), and announced with
  `aria-keyshortcuts` on a hidden marker so the key is discoverable. `on.trigger` runs through the view core.
-->
<script lang="ts">
  import { ariaShortcut } from '../core/shortcuts'
  import { useView } from '../context'
  import { commonOf, needsHost, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx

  const common = $derived(commonOf(ctx, node, path, scope))
  const spec = $derived(str(view.evaluate(node.props?.shortcut, scope)))
  const blocked = $derived(common.disabled || (view.store.link !== 'online' && needsHost(node.on?.trigger)))

  $effect(() => {
    if (!spec) return undefined
    return view.shortcuts.register(spec, () => view.fire(node, path, 'trigger', scope), () => !blocked)
  })
</script>

<span hidden data-testid={common.testid} aria-keyshortcuts={ariaShortcut(spec)}></span>
