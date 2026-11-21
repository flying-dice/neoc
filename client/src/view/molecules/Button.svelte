<!--
  @molecule Button — wire props to the kit Button. `on.tap` runs through the view core: client actions first (no
  round trip), `emit` towards the app; `on.longPress` fires after a press held for 500 ms (and replaces the tap
  for that press). Roles: `primary` is the filled button, `destructive` is red, `cancel` is the quiet gray one.
  `fill` is a data colour (palette token or `#rrggbb`) with auto-contrast text. `selected` is `aria-pressed`,
  `loading` shows a spinner and disables it. Offline, a button that needs the host is disabled. A `shortcut`
  registers a window-scoped key command and is announced with `aria-keyshortcuts`. Atoms: Button, Icon,
  ActivityIndicator, Children.
-->
<script lang="ts">
  import { ActivityIndicator, Button, Icon } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { iconOf } from '../core/menuItems'
  import { ariaShortcut } from '../core/shortcuts'
  import { useView } from '../context'
  import { bool, commonOf, needsHost, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle, optStr } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const LONG_PRESS_MS = 500

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const iconName = $derived(iconOf(p('icon')))
  const role = $derived(
    oneOf(p('role'), ['normal', 'primary', 'destructive', 'cancel'] as const, 'normal')
  )
  const fill = $derived(optStr(p('fill')))
  const variant = $derived.by(() => {
    const style = oneOf(
      p('style'),
      ['filled', 'tinted', 'gray', 'plain', 'bordered'] as const,
      'tinted'
    )
    if (p('style') !== undefined) return style === 'bordered' ? 'gray' : style
    if (role === 'primary' || fill) return 'filled'
    if (role === 'cancel') return 'gray'
    return 'tinted'
  })
  const size = $derived(oneOf(p('size'), ['small', 'medium', 'large', 'fill'] as const, 'medium'))
  const shape = $derived(oneOf(p('shape'), ['rounded', 'capsule', 'circle'] as const, 'rounded'))
  const loading = $derived(bool(p('loading')))
  const selected = $derived(p('selected') === undefined ? undefined : bool(p('selected')))
  const offlineBlocked = $derived(view.store.link !== 'online' && needsHost(node.on?.tap))
  const disabled = $derived(common.disabled || loading || offlineBlocked)
  const name = $derived(common.label ?? (title || undefined))
  const hasCustomLabel = $derived((node.children?.length ?? 0) > 0)
  const appearance = $derived(appearanceStyle(ctx, node, scope))
  const hasLongPress = $derived(node.on?.longPress !== undefined)

  let pressTimer: ReturnType<typeof setTimeout> | undefined
  let longPressed = false

  function tap(): void {
    if (!disabled) view.fire(node, path, 'tap', scope)
  }

  function onpointerdown(): void {
    longPressed = false
    if (!hasLongPress || disabled) return
    pressTimer = setTimeout(() => {
      longPressed = true
      view.fire(node, path, 'longPress', scope)
    }, LONG_PRESS_MS)
  }
  function endPress(): void {
    clearTimeout(pressTimer)
  }
  /** A press that became a long press is not also a tap. */
  function onclick(e: MouseEvent): void {
    if (longPressed) {
      longPressed = false
      e.preventDefault()
      return
    }
    tap()
  }

  $effect(() => {
    const spec = common.shortcut
    if (!spec) return undefined
    return view.shortcuts.register(spec, tap, () => !disabled)
  })
  $effect(() => endPress)
</script>

<Button
  testid={common.testid}
  {variant}
  {size}
  capsule={shape === 'capsule' || shape === 'circle'}
  square={shape === 'circle'}
  destructive={role === 'destructive'}
  {fill}
  {disabled}
  aria-label={name}
  aria-description={common.hint}
  aria-pressed={selected}
  aria-busy={loading || undefined}
  aria-keyshortcuts={common.shortcut ? ariaShortcut(common.shortcut) : undefined}
  title={common.help}
  style="{common.style};{appearance}"
  {onclick}
  {onpointerdown}
  onpointerup={endPress}
  onpointerleave={endPress}
  onpointercancel={endPress}
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
