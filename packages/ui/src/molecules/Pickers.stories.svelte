<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import ColorPicker from './ColorPicker.svelte'
  import DatePicker from './DatePicker.svelte'
  import Label from './Label.svelte'
  import OptionList from './OptionList.svelte'
  import SearchField from './SearchField.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/Pickers and fields',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })

  const changed = fn()
  const submitted = fn()
  const cancelled = fn()
</script>

<script lang="ts">
  let date = $state('2026-11-18')
  let time = $state('09:30')
  let both = $state('2026-11-18T09:30:00Z')
  let graphical = $state('2026-11-18')
  let ranged = $state('2026-11-18')
  let color = $state('blue')
  let custom = $state('#1d3557')
  let choice = $state(['week'])
  let many = $state(['a', 'c'])
  let query = $state('')
</script>

<Story name="Date picker, compact" asChild>
  <div style="display:grid;gap:14px;justify-items:start">
    <DatePicker testid="date" label="Due date" bind:value={date} timeZone="UTC" locale="en-GB" onchange={changed} />
    <DatePicker testid="time" label="Reminder time" kind="time" step={15} bind:value={time} timeZone="UTC" />
    <DatePicker testid="both" label="Starts" kind="dateTime" bind:value={both} timeZone="UTC" />
    <DatePicker testid="disabled" label="Locked" value="2026-01-01" disabled timeZone="UTC" />
  </div>
</Story>

<Story name="Date picker, graphical" asChild>
  <DatePicker testid="date" label="Due date" style="graphical" bind:value={graphical} timeZone="UTC" locale="en-GB" />
</Story>

<Story name="Date and time, graphical" asChild>
  <DatePicker testid="both" label="Starts" kind="dateTime" style="graphical" bind:value={both} timeZone="UTC" locale="en-GB" step={5} />
</Story>

<Story name="Date picker with min and max" asChild>
  <DatePicker testid="date" label="Due date" style="graphical" bind:value={ranged} min="2026-11-10" max="2026-11-25" timeZone="UTC" locale="en-GB" />
</Story>

<Story
  name="Calendar by keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const day = (d: string): HTMLElement => canvas.getByTestId(`date-day-${d}`)
    day('2026-11-18').focus()
    await userEvent.keyboard('{ArrowRight}')
    await expect(day('2026-11-19')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(day('2026-11-26')).toHaveFocus()
    await userEvent.keyboard('{Home}')
    await expect(day('2026-11-23')).toHaveFocus() // the Monday of that week
    await userEvent.keyboard('{PageDown}')
    await waitFor(() => expect(canvas.getByTestId('date-day-2026-12-23')).toHaveFocus())
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(changed).toHaveBeenCalledWith('2026-12-23'))
    await expect(canvas.getByTestId('date-day-2026-12-23')).toHaveAttribute('aria-label', expect.stringContaining('23 December'))
    await userEvent.click(canvas.getByTestId('date-prev'))
    await waitFor(() => expect(canvas.getByTestId('date-day-2026-11-30')).toBeVisible())
  }}
>
  <DatePicker testid="date" label="Due date" style="graphical" value="2026-11-18" timeZone="UTC" locale="en-GB" onchange={changed} />
</Story>

<Story name="Colour picker" asChild>
  <div style="width:300px;display:grid;gap:16px">
    <ColorPicker testid="color" label="Label colour" bind:value={color} onchange={changed} />
    <ColorPicker testid="color-custom" label="Accent" bind:value={custom} custom opacity palette={['blue', 'green', 'orange', 'pink']} />
  </div>
</Story>

<Story
  name="Colour picker by keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.tab()
    await expect(canvas.getByTestId('color-swatch-blue')).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}')
    await waitFor(() => expect(canvas.getByTestId('color-swatch-red')).toHaveAttribute('aria-checked', 'true'))
    await userEvent.keyboard('{End}')
    await waitFor(() => expect(canvas.getByTestId('color-swatch-gray6')).toHaveAttribute('aria-checked', 'true'))
    await userEvent.type(canvas.getByTestId('color-hex'), 'ff8800')
    await waitFor(() => expect(canvas.getByTestId('color-hex')).toHaveValue('ff8800'))
  }}
>
  <div style="width:300px"><ColorPicker testid="color" label="Label colour" bind:value={color} custom /></div>
</Story>

<Story name="Option list (inline picker)" asChild>
  <div style="width:300px;display:grid;gap:16px">
    <OptionList
      testid="options"
      label="Range"
      options={[{ key: 'day', label: 'Day' }, { key: 'week', label: 'Week' }, { key: 'month', label: 'Month', icon: 'clock' }, { key: 'year', label: 'Year', disabled: true }]}
      selected={choice}
      onselect={(k) => (choice = [k])}
    />
    <OptionList
      testid="options-multi"
      label="Notify me about"
      multiple
      options={[{ key: 'a', label: 'Mentions' }, { key: 'b', label: 'Replies' }, { key: 'c', label: 'Reminders' }]}
      selected={many}
      onselect={(k) => (many = many.includes(k) ? many.filter((x) => x !== k) : [...many, k])}
    />
  </div>
</Story>

<Story
  name="Option list by keyboard"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.tab()
    await expect(canvas.getByTestId('options-option-week')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(canvas.getByTestId('options-option-month')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}') // skips the disabled year
    await expect(canvas.getByTestId('options-option-day')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(canvas.getByTestId('options-option-day')).toHaveAttribute('aria-selected', 'true'))
    await userEvent.keyboard('m')
    await expect(canvas.getByTestId('options-option-month')).toHaveFocus()
  }}
>
  <div style="width:300px">
    <OptionList
      testid="options"
      label="Range"
      options={[{ key: 'day', label: 'Day' }, { key: 'week', label: 'Week' }, { key: 'month', label: 'Month' }, { key: 'year', label: 'Year', disabled: true }]}
      selected={choice}
      onselect={(k) => (choice = [k])}
    />
  </div>
</Story>

<Story name="Search field" asChild>
  <div style="width:300px;display:grid;gap:12px">
    <SearchField testid="search" bind:value={query} showCancel />
    <SearchField testid="search-small" size="small" value="swift" />
    <SearchField testid="search-disabled" value="" disabled />
  </div>
</Story>

<Story
  name="Search field submit and cancel"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const input = canvas.getByTestId('search')
    await userEvent.type(input, 'notes{Enter}')
    await waitFor(() => expect(submitted).toHaveBeenCalledWith('notes'))
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(input).toHaveValue(''))
    await expect(cancelled).toHaveBeenCalled()
  }}
>
  <div style="width:300px"><SearchField testid="search" bind:value={query} onsubmit={submitted} oncancel={cancelled} showCancel /></div>
</Story>

<Story name="Label" asChild>
  <div style="display:grid;gap:10px">
    <Label testid="label-both" title="Archive" icon="folder" />
    <Label testid="label-title" title="Title only" icon="folder" display="titleOnly" />
    <Label testid="label-icon" title="Delete" icon="trash" display="iconOnly" color="destructive" />
    <Label testid="label-secondary" title="Updated today" icon="clock" color="secondary" style="subheadline" />
  </div>
</Story>
