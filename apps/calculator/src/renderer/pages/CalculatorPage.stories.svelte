<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import CalculatorPage from './CalculatorPage.svelte'
  import { calculatorApi, manifest } from '../../testing'

  /** The Calculator window, running its real main module in memory. */
  const { Story } = defineMeta({
    title: 'Apps/Calculator/Calculator',
    component: CalculatorPage,
    tags: ['app'],
    args: { width: manifest.window.width, height: manifest.window.height },
    argTypes: {
      width: { control: { type: 'range', min: 320, max: 900, step: 10 } },
      height: { control: { type: 'range', min: 240, max: 900, step: 10 } }
    },
    parameters: { layout: 'centered' },
    render: template
  })

  const sizeOf = (args: { width: number; height: number }) => ({
    width: args.width,
    height: args.height,
    minWidth: manifest.window.minWidth,
    minHeight: manifest.window.minHeight
  })

  type Canvas = { getByTestId(id: string): HTMLElement }
  type Clicker = { click(el: HTMLElement): Promise<void> }

  async function press(canvas: Canvas, userEvent: Clicker, ...keys: string[]): Promise<void> {
    for (const key of keys) await userEvent.click(canvas.getByTestId(`calculator-key-${key}`))
    // A pointer click wouldn't leave a focus ring; programmatic clicks do.
    ;(document.activeElement as HTMLElement | null)?.blur()
  }
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title="Calculator" {...sizeOf(args)} api={calculatorApi()}>
    <CalculatorPage />
  </AppWindow>
{/snippet}

<Story name="Default" />

<Story
  name="Entering an operation"
  play={async ({ canvas, userEvent }) => {
    await press(canvas, userEvent, '1', '2', 'add', '7')
    await expect(canvas.getByTestId('calculator-display')).toHaveTextContent('7')
    await expect(canvas.getByTestId('calculator-key-add')).toHaveAttribute('aria-pressed', 'false')
  }}
/>

<Story
  name="Operator pending"
  play={async ({ canvas, userEvent }) => {
    await press(canvas, userEvent, '1', '2', 'multiply')
    await expect(canvas.getByTestId('calculator-key-multiply')).toHaveAttribute('aria-pressed', 'true')
  }}
/>

<Story
  name="Result"
  play={async ({ canvas, userEvent }) => {
    await press(canvas, userEvent, '1', '2', 'add', '7', 'equals')
    await expect(canvas.getByTestId('calculator-display')).toHaveTextContent('19')
  }}
/>

<Story
  name="Long number"
  play={async ({ canvas, userEvent }) => {
    await press(canvas, userEvent, '9', '8', '7', '6', '5', '4', '3', '2', '1', 'multiply', '9', '9', '9', 'equals')
    await expect(canvas.getByTestId('calculator-display')).toHaveTextContent('9.86666667e11')
  }}
/>

<Story
  name="Division by zero"
  play={async ({ canvas, userEvent }) => {
    await press(canvas, userEvent, '8', 'divide', '0', 'equals')
    await expect(canvas.getByTestId('calculator-display')).toHaveTextContent('Error')
  }}
/>

<Story name="Shown by an agent">
  {#snippet template(args: { width: number; height: number })}
    <AppWindow
      title="Calculator"
      {...sizeOf(args)}
      api={calculatorApi((host) => host.emit('calculator', 'display', { expression: '2450 * 0.18', result: 441 }))}
    >
      <CalculatorPage />
    </AppWindow>
  {/snippet}
</Story>
