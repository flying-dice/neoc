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
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'
  import type { WindowLayout } from '../types'

  interface Props {
    testid: string
    layout: WindowLayout
    title?: string
    onlayout?: (layout: WindowLayout) => void
    onminimize?: () => void
    onclose?: () => void
    /** Escape or Tab: the person leaves the menu without choosing. */
    ondismiss?: (how: 'escape' | 'tab') => void
  }

  let { testid, layout, title, onlayout, onminimize, onclose, ondismiss }: Props = $props()

  let menu: HTMLElement | undefined = $state()
  /** The item that holds the tab stop (the one last focused; the first until then). */
  let current = $state(0)

  const items = (): HTMLElement[] => [...(menu?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? [])]

  function focusItem(index: number): void {
    const all = items()
    all[Math.min(Math.max(0, index), all.length - 1)]?.focus()
  }
  /** Whatever takes focus (the arrows, a click) holds the tab stop. */
  function onfocusin(e: FocusEvent): void {
    const at = items().indexOf(e.target as HTMLElement)
    if (at >= 0) current = at
  }

  // A menu that opens takes the focus, on its first item.
  $effect(() => {
    if (menu) focusItem(0)
  })

  const STEP: Record<string, number> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }

  function onkeydown(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    const at = items().indexOf(document.activeElement as HTMLElement)
    if (e.key in STEP) focusItem((at < 0 ? current : at) + STEP[e.key])
    else if (e.key === 'Home') focusItem(0)
    else if (e.key === 'End') focusItem(items().length - 1)
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
        <button
          type="button"
          role="menuitemradio"
          data-testid="{testid}-{o.value}"
          aria-checked={layout === o.value}
          tabindex={current === i ? 0 : -1}
          aria-label={o.label}
          title={o.label}
          class="layout"
          class:current={layout === o.value}
          onclick={() => onlayout?.(o.value)}
        >
          <Icon name={o.icon} size={26} weight={1.8} />
        </button>
      {/each}
    </div>
    <div class="actions">
      <button
        type="button"
        role="menuitem"
        class="action"
        tabindex={current === options.length ? 0 : -1}
        data-testid="{testid}-minimize"
        onclick={onminimize}
      >
        <span>Minimize</span><Icon name="minus" size={17} weight={2.2} />
      </button>
      <button
        type="button"
        role="menuitem"
        class="action destructive"
        tabindex={current === options.length + 1 ? 0 : -1}
        data-testid="{testid}-close"
        onclick={onclose}
      >
        <span>Close</span><Icon name="xmark" size={15} weight={2.4} />
      </button>
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
