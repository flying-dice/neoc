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
