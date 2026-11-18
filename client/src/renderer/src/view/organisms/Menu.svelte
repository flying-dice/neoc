<!--
  @organism Menu — a trigger button (`title` and/or `icon`) that opens a menu of its children: Buttons, Toggles
  (checkmarks), Dividers, an inline Picker (its choices, the chosen one checked), nested Menus and Sections
  (headings). With `on.primaryAction` the button itself runs that action and a small chevron next to it opens the
  menu. Keyboard: Enter, Space or ArrowDown on the trigger opens it and focuses the first item; Up/Down/Home/End,
  type-ahead, Right/Left for submenus, Escape closes and returns focus to the trigger. Molecules: Menu, Popover.
  Atoms: Button, Icon.
-->
<script lang="ts">
  import { Button, Icon } from '@agentos/ui/atoms'
  import { Menu, Popover } from '@agentos/ui/molecules'
  import { iconOf, menuItems } from '../core/menuItems'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { popupMenu } from '../kit/popupMenu.svelte'
  import { portalToWindow } from '../overlay'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const popup = popupMenu()
  const apple = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const icon = $derived(iconOf(p('icon')))
  const name = $derived(common.label ?? (title || undefined))
  const items = $derived(popup.open ? menuItems({ view, scope, apple }, node.children, path) : [])
  const split = $derived(node.on?.primaryAction !== undefined)
</script>

<span class="menu" class:split style={common.style} title={common.help}>
  {#if split}
    <Button
      testid="{common.testid}-primary"
      variant="tinted"
      disabled={common.disabled}
      aria-label={name}
      onclick={() => view.fire(node, path, 'primaryAction', scope)}
    >
      {#if icon}<Icon name={icon} size={16} weight={2.2} />{/if}
      {title}
    </Button>
  {/if}
  <Button
    testid={common.testid}
    variant={split ? 'gray' : icon && !title ? 'plain' : 'gray'}
    square={split || (!!icon && !title)}
    disabled={common.disabled}
    aria-label={split ? `${name ?? 'Menu'}, more actions` : name}
    aria-description={common.hint}
    aria-haspopup="menu"
    aria-expanded={popup.open}
    onclick={() => popup.toggle()}
    onkeydown={(e) => popup.onTriggerKey(e)}
    {@attach (el: HTMLElement) => {
      popup.trigger = el
    }}
  >
    {#if split}
      <Icon name="chevron.down" size={14} weight={2.6} />
    {:else}
      {#if icon}<Icon name={icon} size={icon && !title ? 20 : 16} weight={2.2} />{/if}
      {#if title}{title}<Icon name="chevron.down" size={12} weight={2.8} />{/if}
    {/if}
  </Button>
</span>

<div class="portal" {@attach portalToWindow(ctx.app)}>
<Popover
  testid="{common.testid}-popover"
  kind="menu"
  open={popup.open}
  anchor={popup.trigger ?? null}
  arrow={false}
  edge="bottom"
  onclose={() => popup.close()}
>
  <Menu
    testid="{common.testid}-menu"
    variant="ios"
    label={name}
    {items}
    autofocus
    onselect={() => popup.close()}
    onclose={() => popup.close()}
  />
</Popover>
</div>

<style>
  .portal {
    display: contents;
  }
  .menu {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
</style>
