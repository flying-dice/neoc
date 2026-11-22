<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor } from 'storybook/test'
  import ContextMenu from './ContextMenu.svelte'
  import AppIcon from './AppIcon.svelte'
  import type { AnchorRect, MenuItem } from '../types'

  const { Story } = defineMeta({
    title: 'Molecules/ContextMenu',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })

  const items: MenuItem[] = [
    { id: 'open', label: 'Open', icon: 'arrow.up.forward.square' },
    { id: 'pin', label: 'Add to Dock', icon: 'dock.rectangle' },
    { id: 'remove', label: 'Delete App', icon: 'trash', destructive: true }
  ]

  const chosen = fn()
  const dismissed = fn()

  type Canvas = {
    findByTestId: (id: string) => Promise<HTMLElement>
    getByTestId: (id: string) => HTMLElement
    queryByTestId: (id: string) => HTMLElement | null
  }
  const gone = (canvas: Canvas): Promise<void> => waitFor(() => expect(canvas.queryByTestId('menu')).toBeNull())
</script>

<script lang="ts">
  /** The menu that is open: the icon it was asked for on, and where that icon is. */
  let menu = $state<{ anchor: HTMLElement; rect: AnchorRect } | null>(null)

  function ask(anchor: HTMLElement): void {
    const { left, top, width, height } = anchor.getBoundingClientRect()
    menu = { anchor, rect: { left, top, width, height } }
  }
  /** As an owner does: the menu goes, and focus returns to the icon. */
  function close(): void {
    const anchor = menu?.anchor
    menu = null
    anchor?.focus()
  }
</script>

{#snippet screen(place: 'top' | 'bottom')}
  <div class="screen" class:bottom={place === 'bottom'} data-testid="screen">
    <AppIcon testid="icon" name="Calculator" icon="calculator" background="linear-gradient(180deg, #ffa733, #ff9500)" onmenu={ask} />
    {#if menu}
      <ContextMenu
        testid="menu"
        label="Calculator"
        {items}
        anchor={menu.rect}
        onselect={(id) => {
          chosen(id)
          close()
        }}
        ondismiss={(how) => {
          dismissed(how)
          close()
        }}
      />
    {/if}
  </div>
{/snippet}

<!-- A secondary click on the icon opens its menu beside it: below, centred on the icon, with focus on the first item. -->
<Story
  name="Open"
  asChild
  play={async ({ canvas }) => {
    const icon = await canvas.findByTestId('icon')
    await expect(icon).toHaveAttribute('aria-haspopup', 'menu')
    await userEvent.pointer({ keys: '[MouseRight]', target: icon })
    const menu = await canvas.findByTestId('menu')
    await expect(menu).toHaveAccessibleName('Calculator')
    await expect(menu).toHaveAttribute('role', 'menu')
    await waitFor(() => expect(canvas.getByTestId('menu-open')).toHaveFocus())
    // below the icon, and wholly on the screen
    const box = menu.getBoundingClientRect()
    const screen = canvas.getByTestId('screen').getBoundingClientRect()
    await expect(box.top).toBeGreaterThanOrEqual(icon.getBoundingClientRect().bottom)
    await expect(box.left).toBeGreaterThanOrEqual(screen.left)
    await expect(box.right).toBeLessThanOrEqual(screen.right)
    await expect(box.bottom).toBeLessThanOrEqual(screen.bottom)
  }}
>
  {@render screen('top')}
</Story>

<!-- An icon at the bottom of the screen (the Dock): there is no room below it, so the menu opens above. -->
<Story
  name="Above, at the bottom of the screen"
  asChild
  play={async ({ canvas }) => {
    const icon = await canvas.findByTestId('icon')
    await userEvent.pointer({ keys: '[MouseRight]', target: icon })
    const menu = await canvas.findByTestId('menu')
    await waitFor(() => expect(menu.getBoundingClientRect().bottom).toBeLessThanOrEqual(icon.getBoundingClientRect().top))
    await expect(menu.getBoundingClientRect().top).toBeGreaterThanOrEqual(canvas.getByTestId('screen').getBoundingClientRect().top)
  }}
>
  {@render screen('bottom')}
</Story>

<!--
  The keyboard: Shift+F10 (or the Menu key) on the icon opens the menu with focus on its first item; Down and Up move
  and stop at the ends, End and Home jump; one item is in the tab order; Escape closes it and focus is back on the icon.
  Enter chooses.
-->
<Story
  name="Keyboard"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    chosen.mockClear()
    dismissed.mockClear()
    const icon = await canvas.findByTestId('icon')
    icon.focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await canvas.findByTestId('menu')
    const item = (id: string): HTMLElement => canvas.getByTestId(`menu-${id}`)
    await waitFor(() => expect(item('open')).toHaveFocus())
    await userEvent.keyboard('{ArrowUp}') // the ends do not wrap
    await expect(item('open')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(item('pin')).toHaveFocus()
    await expect(item('pin')).toHaveAttribute('tabindex', '0')
    await expect(item('open')).toHaveAttribute('tabindex', '-1')
    await userEvent.keyboard('{End}')
    await expect(item('remove')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(item('remove')).toHaveFocus()
    await userEvent.keyboard('{Home}')
    await expect(item('open')).toHaveFocus()

    await userEvent.keyboard('{Escape}')
    await gone(canvas)
    await expect(dismissed).toHaveBeenCalledWith('escape')
    await expect(icon).toHaveFocus()

    // the Menu key opens it too; Enter chooses the focused item
    await userEvent.keyboard('{ContextMenu}')
    await canvas.findByTestId('menu')
    await waitFor(() => expect(item('open')).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}{Enter}')
    await gone(canvas)
    await expect(chosen).toHaveBeenCalledWith('pin')
    await expect(icon).toHaveFocus()

    // Tab leaves the menu as Escape does
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await canvas.findByTestId('menu')
    await waitFor(() => expect(item('open')).toHaveFocus())
    await userEvent.tab()
    await gone(canvas)
    await expect(dismissed).toHaveBeenLastCalledWith('tab')
  }}
>
  {@render screen('top')}
</Story>

<!-- The pointer: a press anywhere outside the menu closes it and presses nothing else; a click on an item chooses it. -->
<Story
  name="Pointer"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    chosen.mockClear()
    dismissed.mockClear()
    const icon = await canvas.findByTestId('icon')
    await userEvent.pointer({ keys: '[MouseRight]', target: icon })
    await canvas.findByTestId('menu')
    await userEvent.pointer({ keys: '[MouseLeft]', target: canvas.getByTestId('menu-layer') })
    await gone(canvas)
    await expect(dismissed).toHaveBeenCalledWith('outside')
    await expect(chosen).not.toHaveBeenCalled()
    await expect(icon).toHaveFocus()

    await userEvent.pointer({ keys: '[MouseRight]', target: icon })
    await userEvent.click(await canvas.findByTestId('menu-remove'))
    await gone(canvas)
    await expect(chosen).toHaveBeenCalledWith('remove')
  }}
>
  {@render screen('top')}
</Story>

<!-- A finger: resting on the icon for half a second opens the menu, and lifting it does not also tap the icon. -->
<Story
  name="Long press"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    const icon = await canvas.findByTestId('icon')
    const touch = { bubbles: true, pointerType: 'touch', pointerId: 7, clientX: 40, clientY: 40 }
    // a finger that moves away (a scroll) is not a long press
    icon.dispatchEvent(new PointerEvent('pointerdown', touch))
    icon.dispatchEvent(new PointerEvent('pointermove', { ...touch, clientY: 80 }))
    await new Promise((r) => setTimeout(r, 650))
    await expect(canvas.queryByTestId('menu')).toBeNull()
    icon.dispatchEvent(new PointerEvent('pointerup', touch))

    icon.dispatchEvent(new PointerEvent('pointerdown', touch))
    await expect(canvas.queryByTestId('menu')).toBeNull() // not at once
    await canvas.findByTestId('menu', undefined, { timeout: 2000 })
    icon.dispatchEvent(new PointerEvent('pointerup', touch))
  }}
>
  {@render screen('top')}
</Story>

<style>
  .screen {
    position: relative;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    width: 420px;
    height: 320px;
    padding: 24px;
    overflow: hidden;
    border-radius: var(--radius-xl);
    background: linear-gradient(135deg, #6e8efb, #c471ed);
  }
  .screen.bottom {
    align-items: flex-end;
  }
</style>
