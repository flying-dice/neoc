<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import Display from './Display.svelte'

  /** The calculator's display: the number, and the line over it. Drawn on the instrument's dark face, one column wide. */
  const { Story } = defineMeta({
    title: 'Molecules/Display',
    component: Display,
    tags: ['autodocs'],
    args: { display: '0', caption: '' },
    render: template
  })

  const fontSize = (el: HTMLElement): number => parseFloat(getComputedStyle(el).fontSize)
</script>

{#snippet template(args: { display: string; caption: string })}
  <div
    data-theme="dark"
    style="width:296px;padding:12px;display:flex;flex-direction:column;gap:8px;background:var(--bg);color:var(--label)"
  >
    <Display {...args} />
  </div>
{/snippet}

<!-- A calculator that has just opened: 0, and an empty caption that still takes its line. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('calculator-display')).toHaveTextContent('0')
    await expect(canvas.getByTestId('calculator-display')).toHaveAccessibleName('Result 0')
    await expect(canvas.getByTestId('calculator-caption').getBoundingClientRect().height).toBeGreaterThan(0)
  }}
/>

<Story name="A number being typed" args={{ display: '1,234.5' }} />

<Story name="A sum in progress" args={{ display: '3', caption: '2 + 3 ×' }} />

<Story name="A result" args={{ display: '19', caption: '12 + 7' }} />

<!-- The sum failed: the caption is the reason. -->
<Story name="An error" args={{ display: 'Error', caption: "Can’t divide by zero" }} />

<!-- Nine digits and their separators do not fit at full size: the number shrinks to the column. -->
<Story
  name="A long number shrinks"
  args={{ display: '-123,456,789', caption: '123,456,788 + 1' }}
  play={async ({ canvas }) => {
    const display = canvas.getByTestId('calculator-display')
    await expect(fontSize(display)).toBeLessThan(64)
    await expect(display.scrollWidth).toBeLessThanOrEqual(display.clientWidth)
  }}
/>

<!-- The longest the backend writes: an exponent, under a caption cut with an ellipsis rather than wrapped. -->
<Story
  name="The longest content"
  args={{
    display: '-1.23456789e-100',
    caption: '… 123,456,789 × 987,654,321 ÷ 0.000001 − 123,456,789 + 987,654,321'
  }}
  play={async ({ canvas }) => {
    const display = canvas.getByTestId('calculator-display')
    const caption = canvas.getByTestId('calculator-caption')
    await expect(display.scrollWidth).toBeLessThanOrEqual(display.clientWidth)
    await expect(caption.getBoundingClientRect().height).toBeLessThan(30)
  }}
/>
