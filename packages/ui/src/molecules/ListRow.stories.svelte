<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Toggle from '../atoms/Toggle.svelte'
  import ListRow from './ListRow.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/ListRow',
    component: ListRow,
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })

  const onclick = fn()
  const frame = 'width:360px;background:var(--bg-elevated);border-radius:10px;overflow:hidden'
</script>

<!-- The plain cell the shell uses: a button when it has onclick. -->
<Story name="Plain cell" asChild parameters={{ controls: { disable: true } }}>
  <div style={frame}>
    <ListRow testid="plain-a" title="Wi-Fi" value="Home" icon="wifi" {onclick} />
    <ListRow testid="plain-b" title="About" subtitle="Version 1.0" chevron {onclick} />
    <ListRow testid="plain-c" title="General" icon="gear" selected {onclick} />
  </div>
</Story>

{#snippet trailing()}<Toggle testid="row-toggle" label="Notify me" />{/snippet}
<!-- A row with a control: the row is not a button, so the control is never nested in one. -->
<Story
  name="With a trailing control"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('row').tagName).toBe('DIV')
    const control = canvas.getByRole('switch', { name: 'Notify me' })
    await userEvent.click(control)
    await expect(control).toHaveAttribute('aria-checked', 'true')
  }}
>
  <div style={frame}>
    <ListRow testid="row" title="Reminders" subtitle="Daily at 9" {trailing} />
  </div>
</Story>

<Story name="Narrow, long text truncates" asChild parameters={{ controls: { disable: true } }}>
  <div style="width:280px;background:var(--bg-elevated);border-radius:10px;overflow:hidden">
    <ListRow testid="long" title="A very long title that cannot fit in a narrow window at all" subtitle="And a long subtitle that is cut with an ellipsis too" chevron {onclick} />
  </div>
</Story>
