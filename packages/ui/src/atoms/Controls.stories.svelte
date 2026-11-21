<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fireEvent, fn, userEvent, waitFor, within } from 'storybook/test'
  import Toggle from './Toggle.svelte'
  import Slider from './Slider.svelte'
  import Stepper from './Stepper.svelte'
  import SegmentedControl from './SegmentedControl.svelte'
  import TextArea from './TextArea.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Controls',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })
</script>

<script lang="ts">
  let on = $state(false)
  let box = $state(true)
  let pressed = $state(false)
  let slider = $state(40)
  let ticked = $state(3)
  let count = $state(3)
  let bounded = $state(10)
  let segment = $state<'day' | 'week' | 'month'>('week')
  let text = $state('')
  const spy = fn()
</script>

<Story name="Toggle styles" asChild>
  <div style="display:grid;gap:16px;width:280px">
    <div style="display:flex;justify-content:space-between;align-items:center">
      <span>Switch</span><Toggle testid="toggle-switch" label="Switch" bind:checked={on} />
    </div>
    <div style="display:flex;justify-content:space-between;align-items:center">
      <span>Checkbox</span><Toggle testid="toggle-checkbox" label="Checkbox" variant="checkbox" bind:checked={box} />
    </div>
    <div style="display:flex;justify-content:space-between;align-items:center">
      <span>Button</span><Toggle testid="toggle-button" label="Bold" title="Bold" variant="button" bind:checked={pressed} />
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
    await userEvent.click(canvas.getByTestId('toggle-button'))
    await waitFor(() => expect(canvas.getByTestId('toggle-button')).toHaveAttribute('aria-pressed', 'true'))
  }}
>
  <div style="display:grid;gap:16px;width:280px">
    <Toggle testid="toggle-switch" label="Switch" bind:checked={on} />
    <Toggle testid="toggle-button" label="Bold" title="Bold" variant="button" bind:checked={pressed} />
  </div>
</Story>

<Story name="Slider" asChild>
  <div style="display:grid;gap:20px;width:280px">
    <Slider testid="slider-plain" label="Volume" bind:value={slider} valuetext="{slider} percent" />
    <Slider testid="slider-ticks" label="Quality" min={1} max={5} step={1} ticks={5} bind:value={ticked} />
    <Slider testid="slider-disabled" label="Locked" value={60} disabled />
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
    await expect(slider).toHaveAttribute('aria-valuetext', '70 percent')
  }}
>
  <div style="width:280px;display:grid;gap:8px">
    <Slider testid="slider-keys" label="Volume" valuetext="{slider} percent" bind:value={slider} />
    <span data-testid="slider-keys-value" style="font:var(--type-footnote)">{slider}</span>
  </div>
</Story>

<Story name="Stepper" asChild>
  <div style="display:grid;gap:16px">
    <Stepper testid="stepper-plain" label="Quantity" bind:value={count} />
    <Stepper testid="stepper-bounded" label="Servings" min={1} max={12} step={1} bind:value={bounded} />
    <Stepper testid="stepper-disabled" label="Locked" value={5} disabled />
  </div>
</Story>

<Story
  name="Stepper keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const stepper = canvas.getByTestId('stepper-keys')
    stepper.focus()
    await userEvent.keyboard('{ArrowUp}{ArrowUp}')
    await waitFor(() => expect(stepper).toHaveAttribute('aria-valuenow', '5'))
    await userEvent.keyboard('{End}')
    await waitFor(() => expect(stepper).toHaveAttribute('aria-valuenow', '12'))
    await userEvent.click(canvas.getByTestId('stepper-keys-decrement'))
    await waitFor(() => expect(stepper).toHaveAttribute('aria-valuenow', '11'))
    await userEvent.keyboard('{Home}')
    await waitFor(() => expect(canvas.getByTestId('stepper-keys-decrement')).toBeDisabled())
  }}
>
  <Stepper testid="stepper-keys" label="Servings" min={1} max={12} bind:value={count} />
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
      block
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
      block
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
      onchange={spy}
      block
    />
  </div>
</Story>

<Story name="Text area" asChild>
  <div style="display:grid;gap:12px;width:280px">
    <div style="height:96px;padding:10px 12px;border-radius:var(--radius-md);background:var(--fill-tertiary);display:flex">
      <TextArea testid="textarea-body" aria-label="Note" placeholder="Write something" bind:value={text} />
    </div>
    <div style="height:96px;padding:10px 12px;border-radius:var(--radius-md);background:var(--fill-tertiary);display:flex">
      <TextArea testid="textarea-mono" aria-label="Code" mono value={'const answer = 42\nconsole.log(answer)'} />
    </div>
  </div>
</Story>
