<!--
  @molecule MultitaskingMenu — the iPadOS "···" popover: a row of layout
  choices (Full Screen, Split View, Slide Over, Window) plus window actions.
  Atoms: Material, Icon.
  Test ids: `testid` on the menu, `${testid}-<layout>` on each layout
  (fullscreen, split-left, split-right, slideover, floating), `${testid}-minimize`,
  `${testid}-close`.

  Keyboard (the standard menu pattern): focus moves to the first item when the menu opens; Up/Down (and Left/Right,
  for the row of layouts) move to the previous or next item without wrapping past the ends, Home/End to the first
  or last; one item is in the tab order at a time (roving tabindex). Escape and Tab call `ondismiss` (the owner closes
  the menu and decides where focus goes); Escape does not reach anything behind the menu.

  `unavailable` lists layouts the screen has no room for (Split View on a narrow screen). Such an item is shown dimmed with
  a title and an accessible name saying why, is `aria-disabled` (not `disabled`: it stays in the arrow-key order, as the
  menu pattern asks, so a keyboard or screen reader user finds it and hears why), and does nothing when activated.
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'
  import { moveIndex } from '../keys'
  import type { WindowLayout } from '../types'

  interface Props {
    testid: string
    layout: WindowLayout
    title?: string
    unavailable?: readonly WindowLayout[]
    onlayout?: (layout: WindowLayout) => void
    onminimize?: () => void
    onclose?: () => void
    /** Escape or Tab: the person leaves the menu without choosing. */
    ondismiss?: (how: 'escape' | 'tab') => void
  }

  let { testid, layout, title, unavailable = [], onlayout, onminimize, onclose, ondismiss }: Props = $props()

  const UNAVAILABLE_REASON = 'there is not room for it on this screen'

  let menu: HTMLElement | undefined = $state()
  /** The item that holds the tab stop (the one last focused; the first until then). */
  let tabStop = $state(0)

  const items = (): HTMLElement[] => [...(menu?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? [])]

  function focusItem(index: number): void {
    const all = items()
    all[Math.min(Math.max(0, index), all.length - 1)]?.focus()
  }
  /** Whatever takes focus (the arrows, a click) holds the tab stop. */
  function onfocusin(e: FocusEvent): void {
    const at = items().indexOf(e.target as HTMLElement)
    if (at >= 0) tabStop = at
  }

  // A menu that opens takes the focus, on its first item.
  $effect(() => {
    if (menu) focusItem(0)
  })

  function onkeydown(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    const all = items()
    const at = all.indexOf(document.activeElement as HTMLElement)
    const next = moveIndex(all.length, at < 0 ? tabStop : at, e.key, 'both', false) // the ends do not wrap
    if (next !== null) focusItem(next)
    else if (e.key === 'Escape' || e.key === 'Tab') ondismiss?.(e.key === 'Tab' ? 'tab' : 'escape')
    else return
    e.preventDefault()
    if (e.key === 'Escape') e.stopPropagation() // it closes the menu; nothing behind it hears an Escape
  }

  const options: { value: WindowLayout; label: string; icon: IconName }[] = [
    { value: 'fullscreen', label: 'Full Screen', icon: 'layout.fullscreen' },
    { value: 'split-left', label: 'Split View Left', icon: 'layout.split.left' },
    { value: 'split-right', label: 'Split View Right', icon: 'layout.split.right' },
    { value: 'slideover', label: 'Slide Over', icon: 'layout.slideover' },
    { value: 'floating', label: 'Window', icon: 'layout.window' }
  ]
  const actions = $derived([
    { id: 'minimize', label: 'Minimize', icon: 'minus' as IconName, size: 17, weight: 2.2, run: onminimize },
    { id: 'close', label: 'Close', icon: 'xmark' as IconName, size: 15, weight: 2.4, run: onclose, destructive: true }
  ])
</script>

<!-- svelte-ignore a11y_interactive_supports_focus (the items are the focusable parts of a menu) -->
<div
  class="mt-menu"
  role="menu"
  tabindex="-1"
  aria-label="{title ?? 'Window'} multitasking"
  data-testid={testid}
  bind:this={menu}
  {onkeydown}
  {onfocusin}
>
  <Material thickness="thick" radius="14px">
    <div class="layouts" role="group" aria-label="Layout">
      {#each options as o, i (o.value)}
        {@const off = unavailable.includes(o.value)}
        <button
          type="button"
          role="menuitemradio"
          data-testid="{testid}-{o.value}"
          aria-checked={layout === o.value}
          tabindex={tabStop === i ? 0 : -1}
          aria-label={off ? `${o.label}, unavailable: ${UNAVAILABLE_REASON}` : o.label}
          aria-disabled={off || undefined}
          title={off ? `${o.label}: ${UNAVAILABLE_REASON}` : o.label}
          class="layout"
          class:current={layout === o.value}
          class:unavailable={off}
          onclick={() => !off && onlayout?.(o.value)}
        >
          <Icon name={o.icon} size={26} weight={1.8} />
        </button>
      {/each}
    </div>
    <div class="actions">
      {#each actions as a, i (a.id)}
        <button
          type="button"
          role="menuitem"
          class="action"
          class:destructive={a.destructive}
          tabindex={tabStop === options.length + i ? 0 : -1}
          data-testid="{testid}-{a.id}"
          onclick={a.run}
        >
          <span>{a.label}</span><Icon name={a.icon} size={a.size} weight={a.weight} />
        </button>
      {/each}
    </div>
  </Material>
</div>

<style>
  .mt-menu {
    width: 250px;
    border-radius: 14px;
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.1),
      0 14px 40px rgb(0 0 0 / 0.22);
    color: var(--label);
  }
  .layouts {
    display: flex;
    justify-content: space-between;
    padding: 8px;
  }
  .layout {
    appearance: none;
    border: 0;
    background: transparent;
    width: 44px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    color: var(--label-secondary);
    cursor: pointer;
  }
  .layout:hover {
    background: var(--fill-tertiary);
    color: var(--label);
  }
  .layout.current {
    color: var(--tint);
  }
  /* Unavailable, not hidden: dimmed, no hover, a diagonal strike so it does not rely on the dimming alone. */
  .layout.unavailable {
    color: var(--label-tertiary);
    cursor: default;
    background-image: linear-gradient(
      to top right,
      transparent calc(50% - 1px),
      var(--label-tertiary) calc(50% - 1px),
      var(--label-tertiary) calc(50% + 1px),
      transparent calc(50% + 1px)
    );
  }
  .layout.unavailable:hover {
    background-color: transparent;
    color: var(--label-tertiary);
  }

  .actions {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .action {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 44px;
    padding: 0 16px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--label);
    cursor: pointer;
  }
  .action + .action {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .action:hover {
    background: var(--fill-quaternary);
  }
  .destructive {
    color: var(--ios-red);
  }
</style>
