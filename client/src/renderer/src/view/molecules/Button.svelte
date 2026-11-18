<!--
  @molecule Button — wire props to the kit Button. `on.tap` runs through the view core: client actions first
  (no round trip), `emit` towards the app. Offline, a button that needs the host is disabled. A `shortcut` prop
  registers a window-scoped key command and is announced with `aria-keyshortcuts`. Atoms: Button, Icon,
  ActivityIndicator, Children.
-->
<script lang="ts">
  import { ActivityIndicator, Button, Icon } from '@agentos/ui/atoms'
  import { icons, type IconName } from '@agentos/ui'
  import Children from '../atoms/Children.svelte'
  import { ariaShortcut } from '../core/shortcuts'
  import { useView } from '../context'
  import { bool, commonOf, needsHost, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const iconName = $derived(
    typeof p('icon') === 'string' && (p('icon') as string) in icons
      ? (p('icon') as IconName)
      : undefined
  )
  const variantProp = $derived(
    oneOf(p('style'), ['filled', 'tinted', 'gray', 'plain', 'bordered'] as const, 'tinted')
  )
  const role = $derived(
    oneOf(p('role'), ['normal', 'primary', 'destructive', 'cancel'] as const, 'normal')
  )
  const variant = $derived(
    role === 'primary' && p('style') === undefined
      ? 'filled'
      : variantProp === 'bordered'
        ? 'gray'
        : variantProp
  )
  const size = $derived(oneOf(p('size'), ['small', 'medium', 'large', 'fill'] as const, 'medium'))
  const shape = $derived(oneOf(p('shape'), ['rounded', 'capsule', 'circle'] as const, 'rounded'))
  const loading = $derived(bool(p('loading')))
  const selected = $derived(p('selected') === undefined ? undefined : bool(p('selected')))
  const offlineBlocked = $derived(view.store.link !== 'online' && needsHost(node.on?.tap))
  const disabled = $derived(common.disabled || loading || offlineBlocked)
  const name = $derived(common.label ?? (title || undefined))
  const hasCustomLabel = $derived((node.children?.length ?? 0) > 0)

  const hintAttr = (hint: string | undefined): Record<string, string> =>
    hint ? { 'aria-description': hint } : {}

  function tap(): void {
    if (!disabled) view.fire(node, path, 'tap', scope)
  }

  $effect(() => {
    const spec = common.shortcut
    if (!spec) return undefined
    return view.shortcuts.register(spec, tap, () => !disabled)
  })
</script>

<Button
  testid={common.testid}
  {variant}
  size={size === 'fill' ? 'large' : size}
  block={size === 'fill'}
  capsule={shape === 'capsule' || shape === 'circle'}
  square={shape === 'circle'}
  destructive={role === 'destructive'}
  {disabled}
  aria-label={name}
  {...hintAttr(common.hint)}
  aria-pressed={selected}
  aria-busy={loading || undefined}
  aria-keyshortcuts={common.shortcut ? ariaShortcut(common.shortcut) : undefined}
  title={common.help}
  style={common.style}
  onclick={tap}
>
  {#if loading}
    <ActivityIndicator size="medium" label="Loading" />
  {:else if iconName}
    <Icon name={iconName} size={shape === 'circle' ? 18 : 16} weight={2.2} />
  {/if}
  {#if hasCustomLabel}
    <Children nodes={node.children} {path} {scope} />
  {:else if title}
    {title}
  {/if}
</Button>
