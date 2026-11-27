<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import CalculatorPage from './CalculatorPage.svelte'
  import { Calculator, type CalculatorRpc } from '../lib/calculator.svelte'
  import { createFailingApi, createFakeApi } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's calculator talks to: a fake in the page, never a host. */
    api: CalculatorRpc
  }

  /** The Calculator in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Calculator',
    component: CalculatorPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, api: createFakeApi() },
    argTypes: { api: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement }
  type User = { click(element: Element): Promise<void> }
  const key = (canvas: Canvas, id: string): HTMLElement => canvas.getByTestId(`calculator-key-${id}`)
  /** Click the keys one after another, as a person does. */
  async function tap(canvas: Canvas, user: User, ids: string[]): Promise<void> {
    for (const id of ids) await user.click(key(canvas, id))
  }
  /** The display and the caption, once the answers have come. */
  async function shows(canvas: Canvas, display: string, caption: string): Promise<void> {
    await waitFor(() => expect(canvas.getByTestId('calculator-display')).toHaveTextContent(display))
    await expect(canvas.getByTestId('calculator-display').textContent?.trim()).toBe(display)
    await expect(canvas.getByTestId('calculator-caption').textContent).toBe(caption)
  }
</script>

{#snippet template(args: Args)}
  <!-- one calculator per story, as each window has one -->
  {@const calculator = new Calculator(args.api)}
  <AppWindow
    title={app.title}
    width={args.width}
    height={args.height}
    minWidth={app.window.minW}
    minHeight={app.window.minH}
  >
    <CalculatorPage {calculator} />
  </AppWindow>
{/snippet}

<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await shows(canvas, '0', '')
    await expect(key(canvas, 'clear')).toHaveTextContent('AC')
    await expect(canvas.getByTestId('calculator-announcer')).toHaveTextContent('')
  }}
/>

<!-- Digits are grouped as they are typed, and the clear key becomes C. -->
<Story
  name="A number being typed"
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['1', '2', '3', '4', 'dot', '5'])
    await shows(canvas, '1,234.5', '')
    await expect(key(canvas, 'clear')).toHaveTextContent('C')
  }}
/>

<!-- The operator that waits for its number stays pressed; the caption is the sum so far. -->
<Story
  name="An operator waiting"
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['2', 'add', '3', 'mul'])
    await shows(canvas, '3', '2 + 3 ×')
    await expect(key(canvas, 'mul')).toHaveAttribute('pressed', '')
    await expect(key(canvas, 'add')).not.toHaveAttribute('pressed')
  }}
/>

<Story
  name="A result"
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['1', '2', 'add', '7', 'eq'])
    await shows(canvas, '19', '12 + 7')
    await expect(key(canvas, 'add')).not.toHaveAttribute('pressed')
  }}
/>

<!-- The sum failed: Error, the reason under it, and the reason said to a screen reader. -->
<Story
  name="Divide by zero"
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['8', 'div', '0', 'eq'])
    await shows(canvas, 'Error', "Can't divide by zero")
    await expect(canvas.getByTestId('calculator-announcer')).toHaveTextContent("Can't divide by zero")
  }}
/>

<!-- AC gets out of the error, and the next sum works. -->
<Story
  name="Clear after an error"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['8', 'div', '0', 'eq'])
    await shows(canvas, 'Error', "Can't divide by zero")
    await tap(canvas, userEvent, ['clear'])
    await shows(canvas, '0', '')
    await expect(canvas.getByTestId('calculator-announcer')).toHaveTextContent('')
    await tap(canvas, userEvent, ['4', 'add', '4', 'eq'])
    await shows(canvas, '8', '4 + 4')
  }}
/>

<!-- The key did not reach the backend (the host, the connection): the caption says why, the display keeps what it had,
     and nothing is announced as the sum's failure. -->
<Story
  name="The backend cannot be reached"
  args={{ api: createFailingApi('The host is not answering') }}
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['5'])
    await shows(canvas, '0', 'The host is not answering')
    await expect(canvas.getByTestId('calculator-announcer')).toHaveTextContent('')
  }}
/>

<!-- Nine digits is the most the keypad takes (a tenth is ignored), and the display shrinks to keep them in the column. -->
<Story
  name="A long number"
  play={async ({ canvas, userEvent }) => {
    await tap(canvas, userEvent, ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 'negate'])
    await shows(canvas, '-123,456,789', '')
    const display = canvas.getByTestId('calculator-display')
    await expect(parseFloat(getComputedStyle(display).fontSize)).toBeLessThan(64)
    await expect(display.scrollWidth).toBeLessThanOrEqual(display.clientWidth)
  }}
/>

<!-- The keyboard, with no click first: digits, operators, Enter for =, Backspace, Escape to clear. -->
<Story
  name="Typed on the keyboard"
  play={async ({ canvas, userEvent }) => {
    await userEvent.keyboard('12+79')
    await shows(canvas, '79', '12 +')
    await userEvent.keyboard('{Backspace}')
    await shows(canvas, '7', '12 +')
    await userEvent.keyboard('{Enter}')
    await shows(canvas, '19', '12 + 7')
    await userEvent.keyboard('{Escape}')
    await shows(canvas, '0', '')
    await userEvent.keyboard('250x0,18=')
    await shows(canvas, '45', '250 × 0.18')
  }}
/>

<!-- A key reached with Tab is pressed by Enter, as any button is; Enter after typing is = again. The Tab is pressed, and
     the focus is then given to the key: the testing library's Tab does not look into the kit's shadow trees, where the
     buttons are, so it cannot move the focus there itself. -->
<Story
  name="Enter on a focused key"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.tab()
    key(canvas, '7').focus()
    await expect(key(canvas, '7')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await shows(canvas, '7', '')
    await userEvent.keyboard('+2{Enter}')
    await shows(canvas, '9', '7 + 2')
  }}
/>
