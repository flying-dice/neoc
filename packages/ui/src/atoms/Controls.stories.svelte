<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fireEvent, userEvent, waitFor, within } from 'storybook/test'
  import Toggle from './Toggle.svelte'
  import Slider from './Slider.svelte'
  import SegmentedControl from './SegmentedControl.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Controls',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })
</script>

<script lang="ts">
  let on = $state(false)
  let slider = $state(40)
  let segment = $state<'day' | 'week' | 'month'>('week')
</script>

<Story name="Toggle styles" asChild>
  <div style="display:grid;gap:16px;width:280px">
    <div style="display:flex;justify-content:space-between;align-items:center">
      <span>Switch</span><Toggle testid="toggle-switch" label="Switch" bind:checked={on} />
    </div>
    <div style="display:flex;justify-content:space-between;align-items:center">
      <span>Disabled</span><Toggle testid="toggle-disabled" label="Disabled" checked disabled />
    </div>
  </div>
</Story>

<Story
  name="Toggle keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const sw = canvas.getByTestId('toggle-switch')
    sw.focus()
    await userEvent.keyboard(' ')
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'true'))
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'false'))
  }}
>
  <div style="display:grid;gap:16px;width:280px">
    <Toggle testid="toggle-switch" label="Switch" bind:checked={on} />
  </div>
</Story>

<Story name="Slider" asChild>
  <div style="display:grid;gap:20px;width:280px">
    <Slider testid="slider-plain" label="Volume" bind:value={slider} />
    <Slider testid="slider-steps" label="Quality" min={1} max={5} step={1} value={3} />
  </div>
</Story>

<Story
  name="Slider drives its value"
  asChild
  play={async ({ canvasElement }) => {
    // user-event does not emulate a range input's arrow keys (the browser does), so drive the input event.
    const canvas = within(canvasElement)
    const slider = canvas.getByTestId('slider-keys') as HTMLInputElement
    await fireEvent.input(slider, { target: { value: '70' } })
    await waitFor(() => expect(canvas.getByTestId('slider-keys-value')).toHaveTextContent('70'))
    await expect(slider).toHaveAccessibleName('Volume')
  }}
>
  <div style="width:280px;display:grid;gap:8px">
    <Slider testid="slider-keys" label="Volume" bind:value={slider} />
    <span data-testid="slider-keys-value" style="font:var(--type-footnote)">{slider}</span>
  </div>
</Story>

<Story name="Segmented control" asChild>
  <div style="display:grid;gap:16px;width:280px">
    <SegmentedControl
      testid="segmented"
      label="Range"
      segments={[
        { value: 'day', label: 'Day' },
        { value: 'week', label: 'Week' },
        { value: 'month', label: 'Month' }
      ]}
      bind:value={segment}
    />
    <SegmentedControl
      testid="segmented-disabled-segment"
      label="Range, one unavailable"
      segments={[
        { value: 'day', label: 'Day' },
        { value: 'week', label: 'Week', disabled: true },
        { value: 'month', label: 'Month' }
      ]}
      value="day"
    />
  </div>
</Story>

<Story
  name="Segmented control keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.tab()
    const group = canvas.getByTestId('segmented-keys')
    await expect(canvas.getByTestId('segmented-keys-week')).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}')
    await waitFor(() => expect(canvas.getByTestId('segmented-keys-month')).toHaveAttribute('aria-checked', 'true'))
    await expect(canvas.getByTestId('segmented-keys-month')).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}')
    await waitFor(() => expect(canvas.getByTestId('segmented-keys-day')).toHaveAttribute('aria-checked', 'true'))
    await userEvent.keyboard('{End}')
    await waitFor(() => expect(canvas.getByTestId('segmented-keys-month')).toHaveAttribute('aria-checked', 'true'))
    await expect(group).toBeVisible()
  }}
>
  <div style="width:280px">
    <SegmentedControl
      testid="segmented-keys"
      label="Range"
      segments={[
        { value: 'day', label: 'Day' },
        { value: 'week', label: 'Week' },
        { value: 'month', label: 'Month' }
      ]}
      bind:value={segment}
    />
  </div>
</Story>
