<!--
  @molecule AppIcon — an app's icon (the Dock, the Home Screen): a picture only, named by its `aria-label`.
  Atoms: IconTile, Icon. Test id: `testid`.

  With `onmenu` the icon has a context menu, asked for the three ways iPadOS and a desktop browser do: a secondary
  click (`contextmenu`: right-click, Ctrl+click, a long press where the browser makes one), a long press of a finger or
  pen (half a second without moving; the tap that would follow is dropped), and the keyboard (the Menu key or
  Shift+F10). `onmenu` is given the icon's button: what the menu opens beside, and where focus goes back to. The
  button says so with `aria-haspopup="menu"`.
-->
<script lang="ts">
  import IconTile from '../atoms/IconTile.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'
  import { isContextMenuKey } from '../keys'

  interface Props {
    testid: string
    name: string
    icon: IconName
    /** Any CSS background, typically a two-stop gradient. */
    background?: string
    size?: number
    /** Small dot beneath the icon marking a running app. */
    running?: boolean
    onclick?: () => void
    /** Ask for this icon's context menu; `anchor` is the icon's button. */
    onmenu?: (anchor: HTMLElement) => void
    /** Whether the button is in the tab order: a grid or toolbar that moves focus with the arrows keeps one stop. */
    tabindex?: 0 | -1
  }

  let {
    testid,
    name,
    icon,
    background = 'linear-gradient(180deg, #5ac8fa, #007aff)',
    size = 60,
    running = false,
    onclick,
    onmenu,
    tabindex
  }: Props = $props()

  /** How long a finger rests before the menu opens, and how far it may wander meanwhile (a scroll is not a press). */
  const LONG_PRESS_MS = 500
  const SLOP = 10

  let button: HTMLButtonElement | undefined = $state()
  let press: { x: number; y: number; timer: ReturnType<typeof setTimeout> } | null = null
  /** A long press opened the menu: the click the lifted finger makes is not a tap on the icon. */
  let swallowClick = false

  function cancelPress(): void {
    if (press) clearTimeout(press.timer)
    press = null
  }
  $effect(() => cancelPress)

  function menu(): void {
    if (button) onmenu?.(button)
  }

  function onpointerdown(e: PointerEvent): void {
    swallowClick = false
    if (!onmenu || e.pointerType === 'mouse') return // a mouse asks with its secondary button
    cancelPress()
    const timer = setTimeout(() => {
      press = null
      swallowClick = true
      menu()
    }, LONG_PRESS_MS)
    press = { x: e.clientX, y: e.clientY, timer }
  }
  function onpointermove(e: PointerEvent): void {
    if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > SLOP) cancelPress()
  }
  function onpointerup(): void {
    cancelPress()
    // the lifted finger's click comes at once or not at all: a later press of Enter is the person's own
    if (swallowClick) setTimeout(() => (swallowClick = false), 400)
  }
  function oncontextmenu(e: MouseEvent): void {
    if (!onmenu) return
    e.preventDefault()
    if (press !== null) swallowClick = true // the browser made it from a long press: the lift is not a tap
    cancelPress()
    menu()
  }
  function onkeydown(e: KeyboardEvent): void {
    if (!onmenu || !isContextMenuKey(e)) return
    e.preventDefault() // the browser's own menu, and the `contextmenu` it would send
    menu()
  }
  function click(e: MouseEvent): void {
    if (swallowClick) {
      swallowClick = false
      e.preventDefault()
      return
    }
    onclick?.()
  }
</script>

<button
  type="button"
  class="ios-app-icon"
  class:has-menu={!!onmenu}
  aria-label={name}
  aria-haspopup={onmenu ? 'menu' : undefined}
  data-testid={testid}
  {tabindex}
  bind:this={button}
  onclick={click}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={cancelPress}
  onpointerleave={cancelPress}
  {oncontextmenu}
  {onkeydown}
>
  <span class="plate">
    <IconTile {size} {background} shadow>
      <Icon name={icon} size={Math.round(size * 0.52)} weight={1.9} color="#fff" />
    </IconTile>
  </span>
  {#if running}<span class="running" aria-label="Running"></span>{/if}
</button>

<style>
  .ios-app-icon {
    appearance: none;
    border: 0;
    background: none;
    padding: 0;
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    position: relative;
    transition: transform var(--duration-fast) var(--ease-ios);
  }
  /* a long press opens the menu: not the browser's callout, a text selection or a drag of the picture */
  .ios-app-icon.has-menu {
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
  }
  .ios-app-icon:active {
    transform: scale(0.9);
    filter: brightness(0.85);
  }
  .plate {
    position: relative;
    display: block;
  }
  /* The ring follows the tile's squircle (the button itself is a box that also holds the running dot). */
  .ios-app-icon:focus-visible {
    outline: none;
  }
  .ios-app-icon:focus-visible .plate {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
    border-radius: var(--radius-icon);
  }
  .running {
    position: absolute;
    bottom: -8px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--label-secondary);
  }
</style>
