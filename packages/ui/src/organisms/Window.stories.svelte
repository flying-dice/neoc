<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import Text from '../atoms/Text.svelte'
  import type { Frame } from '../types'
  import Window from './Window.svelte'

  const { Story } = defineMeta({
    title: 'Organisms/Window',
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })

  /** WCAG relative luminance of a computed `rgb(...)` / `rgba(...)` colour. */
  function luminance(css: string): number {
    const [r, g, b] = (css.match(/[\d.]+/g) ?? []).slice(0, 3).map((v) => {
      const c = Number(v) / 255
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
    })
    return 0.2126 * r + 0.7152 * g + 0.0722 * b
  }
  function contrast(a: string, b: string): number {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
    return (hi + 0.05) / (lo + 0.05)
  }
  const gripColour = (grip: HTMLElement): string => getComputedStyle(grip, '::after').borderBottomColor
</script>

<script lang="ts">
  let frame = $state<Frame>({ x: 16, y: 16, width: 360, height: 300 })
  let dark = $state<Frame>({ x: 16, y: 16, width: 360, height: 300 })
  let keyed = $state<Frame>({ x: 16, y: 16, width: 360, height: 300 })
  let capped = $state<Frame>({ x: 16, y: 16, width: 360, height: 300 })
  let typedKeys = $state('')
  let said = $state('')
  let saidUnavail = $state('')
  /** The owner of a window keeps it on the screen: this desk's edge is 40 px short of 460 x 380. */
  const keepOnDesk = (next: Frame): void => {
    capped = { ...next, width: Math.min(next.width, 404), height: Math.min(next.height, 324) }
  }
</script>

<!-- The desk the window floats on: a fixed box so the story measures and snapshots the same every time. -->
{#snippet desk(f: Frame, set: (next: Frame) => void, black: boolean, name: string)}
  <div style="position:relative;width:460px;height:380px;background:var(--ios-gray5);border-radius:var(--radius-xl)">
    <Window
      testid={name}
      title="Sample"
      x={f.x}
      y={f.y}
      width={f.width}
      height={f.height}
      minWidth={320}
      minHeight={240}
      onframechange={set}
    >
      <div
        data-testid="{name}-body"
        style="flex:1;display:grid;place-items:center;{black ? 'background:#000;color:#fff' : ''}"
      >
        <Text style="body" color={black ? 'inherit' : 'secondary'}>{f.width} x {f.height}</Text>
      </div>
    </Window>
  </div>
{/snippet}

<!-- The resize corner on an ordinary body: always visible (no hover), at least 3:1 against the window. -->
<Story
  name="Resize corner"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const grip = await canvas.findByTestId('win-light-resize')
    await expect(grip).toBeVisible()
    const body = getComputedStyle(await canvas.findByTestId('win-light')).backgroundColor
    await expect(contrast(gripColour(grip), body)).toBeGreaterThanOrEqual(3)
  }}
>
  {@render desk(frame, (next) => (frame = next), false, 'win-light')}
</Story>

<!-- The same corner on the always-dark instrument canvas (a black body, as the Calculator has), in either theme. -->
<Story
  name="Resize corner on a black body"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const grip = await canvas.findByTestId('win-dark-resize')
    await expect(grip).toBeVisible()
    await expect(contrast(gripColour(grip), 'rgb(0, 0, 0)')).toBeGreaterThanOrEqual(3)
    const frameBg = getComputedStyle(await canvas.findByTestId('win-dark')).backgroundColor
    await expect(contrast(gripColour(grip), frameBg)).toBeGreaterThanOrEqual(3)
  }}
>
  {@render desk(dark, (next) => (dark = next), true, 'win-dark')}
</Story>

<!-- The keyboard's way to resize: Tab to the grip, then the arrow keys (Shift for 64 px), stopping at the minimum size. -->
<Story
  name="Resize with the keyboard"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const grip = await canvas.findByTestId('win-key-resize')
    await expect(grip).toHaveAccessibleName('Resize Sample')
    await expect(grip).toHaveAttribute('title', 'Resize — drag or use arrow keys')
    await expect(grip).toHaveAccessibleDescription('Drag, or use the arrow keys; Shift for larger steps')
    grip.focus()
    const size = (): string => canvas.getByTestId('win-key-body').textContent?.trim() ?? ''
    await userEvent.keyboard('{ArrowRight}{ArrowRight}')
    await waitFor(() => expect(size()).toBe('392 x 300'))
    await userEvent.keyboard('{Shift>}{ArrowDown}{/Shift}')
    await waitFor(() => expect(size()).toBe('392 x 364'))
    for (let i = 0; i < 20; i++) await userEvent.keyboard('{ArrowLeft}{ArrowUp}')
    await waitFor(() => expect(size()).toBe('320 x 240'))
    await expect(grip).toHaveFocus()
    // the limit label fades after a couple of seconds: the snapshot is of the window after it has gone
    await waitFor(() => expect(canvas.getByTestId('win-key-resize-note')).toHaveTextContent(''), { timeout: 4000 })
    await waitFor(() => expect(canvas.getByTestId('win-key-resize-note')).not.toBeVisible())
  }}
>
  {@render desk(keyed, (next) => (keyed = next), false, 'win-key')}
</Story>

<!-- Reaching the minimum size or the screen's edge: the grip nudges back and says so (once, politely); at its smallest size it stays tinted. -->
<Story
  name="Resize limit cue"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const grip = await canvas.findByTestId('win-limit-resize')
    const status = canvas.getByTestId('win-limit-resize-status')
    const body = getComputedStyle(await canvas.findByTestId('win-limit')).backgroundColor
    const rest = gripColour(grip)
    const size = (): string => canvas.getByTestId('win-limit-body').textContent?.trim() ?? ''
    grip.focus()
    await expect(status).toHaveTextContent('')
    await expect(grip).not.toHaveAttribute('data-limit', 'narrowest')

    // the minimum: nothing is said until a step meets it, then the tint and the message
    await userEvent.keyboard('{ArrowLeft}{ArrowLeft}')
    await waitFor(() => expect(size()).toBe('328 x 300'))
    await expect(status).toHaveTextContent('')
    await userEvent.keyboard('{ArrowLeft}')
    await waitFor(() => expect(size()).toBe('320 x 300'))
    await expect(status).toHaveTextContent("Sample can't get any narrower") // the height has room
    await expect(grip).toHaveAttribute('data-limit', 'narrowest')
    const note = canvas.getByTestId('win-limit-resize-note')
    await expect(note).toHaveTextContent('Narrowest')
    await waitFor(() => expect(note).toBeVisible())
    await waitFor(() => expect(gripColour(grip)).not.toBe(rest))
    await expect(contrast(gripColour(grip), body)).toBeGreaterThanOrEqual(3)
    await expect(contrast(gripColour(grip), 'rgb(0, 0, 0)')).toBeGreaterThanOrEqual(3)
    await userEvent.keyboard('{ArrowLeft}') // pressing on stays at the minimum
    await expect(size()).toBe('320 x 300')
    await expect(grip).toHaveFocus()

    // it lets go once the person stops (and the same message can be heard again later)
    await waitFor(() => expect(status).toHaveTextContent(''), { timeout: 4000 })
    await waitFor(() => expect(gripColour(grip)).toBe(rest), { timeout: 4000 })
    await expect(grip).not.toHaveAttribute('data-limit', 'narrowest')

    // the screen's edge: the owner gives less than was asked for
    await userEvent.keyboard('{Shift>}{ArrowRight}{/Shift}')
    await waitFor(() => expect(size()).toBe('384 x 300'))
    await expect(status).toHaveTextContent('')
    await userEvent.keyboard('{Shift>}{ArrowRight}{/Shift}')
    await waitFor(() => expect(size()).toBe('404 x 300'))
    await waitFor(() => expect(status).toHaveTextContent("Sample can't get any wider here")) // the height has room
    await expect(grip).toHaveAttribute('data-limit', 'widest')
    await waitFor(() => expect(canvas.getByTestId('win-limit-resize-note')).toHaveTextContent('Widest'))
    await waitFor(() => expect(status).toHaveTextContent(''), { timeout: 4000 })

    // the smallest size, both ways: said once, and the grip stays tinted for as long as the window is that small
    for (let i = 0; i < 20; i++) await userEvent.keyboard('{ArrowLeft}{ArrowUp}')
    await waitFor(() => expect(size()).toBe('320 x 240'))
    await waitFor(() => expect(status).toHaveTextContent('Sample is at its smallest size'))
    await waitFor(() => expect(canvas.getByTestId('win-limit-resize-note')).toHaveTextContent('Smallest size'))
    await waitFor(() => expect(status).toHaveTextContent(''), { timeout: 4000 })
    await waitFor(() => expect(canvas.getByTestId('win-limit-resize-note')).not.toBeVisible())
    await expect(grip).not.toHaveAttribute('data-limit', 'smallest')
    await expect(gripColour(grip)).not.toBe(rest) // still tinted, with nothing being said
    await expect(contrast(gripColour(grip), body)).toBeGreaterThanOrEqual(3)
    await expect(grip).toHaveFocus() // the story ends here: a window at its smallest, so the snapshot is the same every time
  }}
>
  <div style="position:relative;width:460px;height:380px;background:var(--ios-gray5);border-radius:var(--radius-xl)">
    <Window
      testid="win-limit"
      title="Sample"
      x={capped.x}
      y={capped.y}
      width={capped.width}
      height={capped.height}
      minWidth={320}
      minHeight={240}
      onframechange={keepOnDesk}
    >
      <div data-testid="win-limit-body" style="flex:1;display:grid;place-items:center">
        <Text style="body" color="secondary">{capped.width} x {capped.height}</Text>
      </div>
    </Window>
  </div>
</Story>

<!--
  The "···" handle takes the app's keys: clicking it or choosing from its menu does not leave the app deaf, and typing
  while the handle itself has focus goes to the app. The content here is what an app window is (`data-window-focus`).
-->
<Story
  name="Keys after the multitasking menu"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const view = await canvas.findByTestId('win-keys-view')
    const typed = (): string => view.textContent?.replace(/^Typed: ?/, '') ?? ''
    const dots = canvas.getByTestId('win-keys-multitasking')
    const menu = (): HTMLElement | null => canvas.queryByTestId('win-keys-menu')

    // a click on the dots leaves focus where it was (not on the button), and Escape closes the menu back to the app
    view.focus()
    await userEvent.click(dots)
    await waitFor(() => expect(menu()).not.toBeNull())
    await expect(dots).not.toHaveFocus()
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(menu()).toBeNull())
    await expect(view).toHaveFocus()
    await userEvent.keyboard('6*7=')
    await waitFor(() => expect(typed()).toBe('6*7='))

    // choosing an item hands the keys back to the app
    await userEvent.click(dots)
    await userEvent.click(await canvas.findByTestId('win-keys-menu-floating'))
    await waitFor(() => expect(menu()).toBeNull())
    await expect(view).toHaveFocus()
    await userEvent.keyboard('9')
    await waitFor(() => expect(typed()).toBe('6*7=9'))

    // minimize too
    await userEvent.click(dots)
    await userEvent.click(await canvas.findByTestId('win-keys-menu-minimize'))
    await expect(view).toHaveFocus()

    // keys typed with focus on the handle itself move to the app and reach it: not lost, not typed twice
    dots.focus()
    await expect(dots).toHaveFocus()
    await userEvent.keyboard('1+')
    await expect(view).toHaveFocus()
    await waitFor(() => expect(typed()).toBe('6*7=91+'))
    dots.focus()
    await userEvent.keyboard('{Backspace}')
    await waitFor(() => expect(typed()).toBe('6*7=91'))

    // Enter is the button's own (it opens the menu), and Escape then gives the button its focus back
    dots.focus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(menu()).not.toBeNull())
    await userEvent.keyboard('{Escape}')
    await expect(dots).toHaveFocus()
    await expect(typed()).toBe('6*7=91')

    // a press on the app while the menu is open closes it and the keys stay with the app
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(menu()).not.toBeNull())
    await userEvent.click(view)
    await waitFor(() => expect(menu()).toBeNull())
    await expect(view).toHaveFocus()
    await userEvent.keyboard('2')
    await waitFor(() => expect(typed()).toBe('6*7=912'))

    // pressing "···" again closes it too, and the keys are not left on the page behind it
    await userEvent.click(dots)
    await waitFor(() => expect(menu()).not.toBeNull())
    await userEvent.click(dots)
    await waitFor(() => expect(menu()).toBeNull())
    await expect(view).toHaveFocus()
    await userEvent.keyboard('3')
    await waitFor(() => expect(typed()).toBe('6*7=9123'))

    // a shortcut with a command key is the browser's: it is not carried to the app
    dots.focus()
    await userEvent.keyboard('{Meta>}r{/Meta}')
    await expect(dots).toHaveFocus()
    await expect(typed()).toBe('6*7=9123')
  }}
>
  <div style="position:relative;width:460px;height:380px;background:var(--ios-gray5);border-radius:var(--radius-xl)">
    <Window testid="win-keys" title="Sample" x={16} y={16} width={360} height={300}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions (an app's window-scoped shortcuts listen on its root) -->
      <div
        role="application"
        aria-label="Typed keys"
        tabindex="-1"
        data-window-focus
        data-testid="win-keys-view"
        style="flex:1;display:grid;place-items:center"
        onkeydown={(e) => (typedKeys = e.key === 'Backspace' ? typedKeys.slice(0, -1) : typedKeys + e.key)}
      >
        <Text style="body" color="secondary">Typed: {typedKeys}</Text>
      </div>
    </Window>
  </div>
</Story>

<!-- The menu's keyboard: focus goes in on the first item, the arrows, Home and End move, Escape and Tab leave. -->
<Story
  name="Multitasking menu keyboard"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const dots = await canvas.findByTestId('win-menu-multitasking')
    const item = (name: string): HTMLElement => canvas.getByTestId(`win-menu-menu-${name}`)
    await expect(dots).toHaveAttribute('aria-haspopup', 'menu')
    await expect(dots).toHaveAttribute('aria-expanded', 'false')
    dots.focus()
    await userEvent.keyboard('{Enter}')
    await expect(dots).toHaveAttribute('aria-expanded', 'true')
    // focus moves into the menu, onto its first item, which alone is in the tab order
    await waitFor(() => expect(item('fullscreen')).toHaveFocus())
    await expect(item('fullscreen')).toHaveAttribute('tabindex', '0')
    await expect(item('close')).toHaveAttribute('tabindex', '-1')

    await userEvent.keyboard('{ArrowRight}{ArrowDown}')
    await expect(item('split-right')).toHaveFocus()
    await userEvent.keyboard('{End}')
    await expect(item('close')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}') // the ends do not wrap
    await expect(item('close')).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}')
    await expect(item('minimize')).toHaveFocus()
    await userEvent.keyboard('{Home}')
    await expect(item('fullscreen')).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}{ArrowLeft}')
    await expect(item('fullscreen')).toHaveFocus()

    // Escape closes it and gives the handle its focus back
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('win-menu-menu')).toBeNull())
    await expect(dots).toHaveFocus()
    await expect(dots).toHaveAttribute('aria-expanded', 'false')

    // Tab leaves it the same way; Space chooses the focused item, as it does on any button
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(item('fullscreen')).toHaveFocus())
    await userEvent.keyboard('{Tab}')
    await waitFor(() => expect(canvas.queryByTestId('win-menu-menu')).toBeNull())
    await expect(dots).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(item('fullscreen')).toHaveFocus())
    await userEvent.keyboard('{ArrowRight}{ArrowRight}{ArrowRight} ')
    await waitFor(() => expect(canvas.getByTestId('win-menu-said')).toHaveTextContent('slideover'))
    await waitFor(() => expect(canvas.queryByTestId('win-menu-menu')).toBeNull())
  }}
>
  <div style="position:relative;width:460px;height:380px;background:var(--ios-gray5);border-radius:var(--radius-xl)">
    <Window testid="win-menu" title="Sample" x={16} y={16} width={360} height={300} onlayout={(l) => (said = l)}>
      <div tabindex="-1" data-window-focus style="flex:1;display:grid;place-items:center">
        <span data-testid="win-menu-said">{said}</span>
      </div>
    </Window>
  </div>
</Story>

<!--
  A layout the screen has no room for (Split View on a narrow screen) is shown, not hidden: dimmed and struck through,
  aria-disabled with the reason in its name and title, still in the arrow-key order, and it does nothing when chosen.
-->
<Story
  name="Unavailable layouts"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    const dots = await canvas.findByTestId('win-unavail-multitasking')
    await userEvent.click(dots)
    const item = (name: string): HTMLElement => canvas.getByTestId(`win-unavail-menu-${name}`)
    for (const name of ['split-left', 'split-right']) {
      await expect(item(name)).toHaveAttribute('aria-disabled', 'true')
      await expect(item(name)).toHaveAccessibleName(/unavailable: there is not room for it on this screen/)
      await expect(item(name)).toHaveAttribute('title', expect.stringContaining('not room'))
    }
    for (const name of ['fullscreen', 'slideover', 'floating']) await expect(item(name)).not.toHaveAttribute('aria-disabled')

    // activating one does nothing (the menu stays open, nothing is chosen); the others still choose
    await userEvent.click(item('split-left'))
    await expect(canvas.getByTestId('win-unavail-said')).toHaveTextContent('')
    await expect(canvas.queryByTestId('win-unavail-menu')).not.toBeNull()

    // it is still reached with the arrow keys, and Enter on it does nothing either
    item('fullscreen').focus()
    await userEvent.keyboard('{ArrowRight}')
    await expect(item('split-left')).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}')
    await expect(item('split-right')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await expect(canvas.getByTestId('win-unavail-said')).toHaveTextContent('')
    await expect(canvas.queryByTestId('win-unavail-menu')).not.toBeNull() // the story ends with it open, for the snapshot
  }}
>
  <div style="position:relative;width:460px;height:380px;background:var(--ios-gray5);border-radius:var(--radius-xl)">
    <Window
      testid="win-unavail"
      title="Sample"
      x={16}
      y={16}
      width={360}
      height={300}
      unavailable={['split-left', 'split-right']}
      onlayout={(l) => (saidUnavail = l)}
    >
      <div tabindex="-1" data-window-focus style="flex:1;display:grid;place-items:center">
        <span data-testid="win-unavail-said">{saidUnavail}</span>
      </div>
    </Window>
  </div>
</Story>
