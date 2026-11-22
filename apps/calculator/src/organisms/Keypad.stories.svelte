<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Keypad from './Keypad.svelte'
  import { KEYS, type ClearLabel, type Key, type Operator } from '../lib/keys'

  interface Args {
    clearLabel: ClearLabel
    pending?: Operator
    onpress: (key: Key) => void
  }

  const onpress = fn<(key: Key) => void>()
  /** The keys the keypad reported, in order. */
  const pressed = (): Key[] => onpress.mock.calls.map(([key]) => key)

  /** The keypad: nineteen kit buttons. It keeps nothing: what the keys say comes in, the key pressed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/Keypad',
    component: Keypad,
    tags: ['autodocs'],
    args: { clearLabel: 'AC', onpress },
    argTypes: {
      clearLabel: { control: 'inline-radio', options: ['AC', 'C'] },
      pending: { control: 'inline-radio', options: [undefined, '+', '-', '*', '/'] }
    },
    beforeEach: () => {
      onpress.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div data-theme="dark" style="display:flex;flex-direction:column;box-sizing:border-box;width:320px;height:380px;padding:12px;background:var(--bg);color:var(--label)">
    <Keypad {...args} />
  </div>
{/snippet}

<!-- Nothing typed: the clear key is AC, and no operator is waiting. -->
<Story
  name="Idle"
  play={async ({ canvas }) => {
    const clear = canvas.getByTestId('calculator-key-clear')
    await expect(clear).toHaveTextContent('AC')
    await expect(clear).toHaveProperty('label', 'AC, all clear') // the kit names its inner button with it
    for (const k of KEYS) await expect(canvas.getByTestId(`calculator-key-${k.id}`)).toBeVisible()
    for (const id of ['add', 'sub', 'mul', 'div']) {
      await expect(canvas.getByTestId(`calculator-key-${id}`)).not.toHaveAttribute('pressed')
    }
  }}
/>

<!-- A number is being typed: the clear key clears the entry only, and says so. -->
<Story
  name="While typing"
  args={{ clearLabel: 'C' }}
  play={async ({ canvas }) => {
    const clear = canvas.getByTestId('calculator-key-clear')
    await expect(clear).toHaveTextContent('C')
    await expect(clear).toHaveProperty('label', 'C, clear entry') // the kit names its inner button with it
  }}
/>

<!-- An operator waits for its second number: its key stays pressed, and only its key. -->
<Story
  name="An operator waiting"
  args={{ pending: '*' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('calculator-key-mul')).toHaveAttribute('pressed', '')
    await expect(canvas.getByTestId('calculator-key-add')).not.toHaveAttribute('pressed')
  }}
/>

<Story
  name="Clicking keys"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    for (const id of ['7', 'mul', 'dot', '0', 'eq', 'negate', 'percent', 'clear']) {
      await userEvent.click(canvas.getByTestId(`calculator-key-${id}`))
    }
    await expect(pressed()).toEqual(['7', '*', '.', '0', '=', 'negate', 'percent', 'clear'])
  }}
/>

<!-- Each key is a button: it takes the focus, and Enter and Space press it. The focus is given to a key here, not moved
     with Tab: the testing library's Tab does not look into the kit's shadow trees, where the buttons are. -->
<Story
  name="By keyboard"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    canvas.getByTestId('calculator-key-clear').focus()
    await expect(canvas.getByTestId('calculator-key-clear')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    canvas.getByTestId('calculator-key-negate').focus()
    await expect(canvas.getByTestId('calculator-key-negate')).toHaveFocus()
    await userEvent.keyboard(' ')
    await expect(pressed()).toEqual(['clear', 'negate'])
  }}
/>
