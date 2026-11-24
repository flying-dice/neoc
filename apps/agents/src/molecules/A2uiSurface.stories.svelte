<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import A2uiSurface from './A2uiSurface.svelte'
  import { surfacesOf, type Surface, type UserAction } from '../lib/a2ui'
  import { SURFACE } from '../lib/fakeApi'

  const onaction = fn<(action: UserAction) => void>()
  const [BOOKING] = surfacesOf(SURFACE)

  /** Every component of A2UI's basic catalog, as 1.0 writes them (`@path`, components inside `createSurface`). */
  const [CATALOG] = surfacesOf({
    a2ui_operations: [
      {
        version: '1.0',
        createSurface: {
          surfaceId: 'catalog',
          dataModel: {
            name: 'Ada',
            agree: true,
            size: ['m'],
            toppings: ['olives'],
            volume: 40,
            when: '2026-06-09',
            guests: [{ name: 'Grace' }, { name: 'Linus' }]
          },
          components: [
            { id: 'root', component: 'Column', children: ['title', 'note', 'row', 'rule', 'name', 'agree', 'size', 'toppings', 'volume', 'when', 'guests', 'tabs', 'more', 'unknown'] },
            { id: 'title', component: 'Text', text: '# The basic catalog' },
            { id: 'note', component: 'Text', variant: 'caption', text: { '@call': 'formatString', args: { value: 'Drawn for ${/name}.' } } },
            { id: 'row', component: 'Row', justify: 'spaceBetween', align: 'center', children: ['icon', 'go'] },
            { id: 'icon', component: 'Icon', name: 'mail' },
            { id: 'go', component: 'Button', variant: 'primary', child: 'go_label', action: { event: { name: 'go', context: { size: { '@path': '/size' }, agree: { '@path': '/agree' } } } } },
            { id: 'go_label', component: 'Text', text: 'Go' },
            { id: 'rule', component: 'Divider' },
            { id: 'name', component: 'TextField', label: 'Name', value: { '@path': '/name' } },
            { id: 'agree', component: 'CheckBox', label: 'I agree', value: { '@path': '/agree' } },
            { id: 'size', component: 'ChoicePicker', label: 'Size', value: { '@path': '/size' }, options: [{ label: 'Small', value: 's' }, { label: 'Medium', value: 'm' }, { label: 'Large', value: 'l' }] },
            { id: 'toppings', component: 'ChoicePicker', label: 'Toppings', variant: 'multipleSelection', value: { '@path': '/toppings' }, options: [{ label: 'Olives', value: 'olives' }, { label: 'Capers', value: 'capers' }] },
            { id: 'volume', component: 'Slider', label: 'Volume', min: 0, max: 100, value: { '@path': '/volume' } },
            { id: 'when', component: 'DateTimeInput', label: 'When', enableDate: true, value: { '@path': '/when' } },
            { id: 'guests', component: 'List', children: { path: '/guests', componentId: 'guest' } },
            { id: 'guest', component: 'Card', child: 'guest_name' },
            { id: 'guest_name', component: 'Text', text: { '@path': 'name' } },
            { id: 'tabs', component: 'Tabs', tabs: [{ title: 'One', child: 'tab_one' }, { title: 'Two', child: 'tab_two' }] },
            { id: 'tab_one', component: 'Text', text: 'The first tab.' },
            { id: 'tab_two', component: 'Text', text: 'The second tab.' },
            { id: 'more', component: 'Modal', trigger: 'more_label', content: 'more_text' },
            { id: 'more_label', component: 'Text', text: 'More' },
            { id: 'more_text', component: 'Text', text: 'What was behind it.' },
            { id: 'unknown', component: 'Hologram' }
          ]
        }
      }
    ]
  })

  /** A segment of a kit segmented control: it is in the control's shadow tree. */
  const segment = (canvas: { getByTestId(id: string): HTMLElement }, control: string, value: string): Element =>
    canvas.getByTestId(control).shadowRoot!.querySelector(`[data-testid="${control}-${value}"]`)!

  /** A surface an agent drew. It keeps what the person entered on it; a pressed button goes out as an action. */
  const { Story } = defineMeta({
    title: 'Molecules/A2uiSurface',
    component: A2uiSurface,
    tags: ['autodocs'],
    args: { surface: BOOKING, onaction },
    beforeEach: () => {
      onaction.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: { surface: Surface; disabled?: boolean; onaction: (action: UserAction) => void })}
  <div style="display:flex;flex-direction:column;width:460px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <A2uiSurface {...args} />
  </div>
{/snippet}

<!-- A2UI 0.9: a field bound to the surface's data, a text made from it, and a button that tells the agent. -->
<Story
  name="A form"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-surface-booking')).toHaveTextContent('A table for 2, for Ada.')
    await userEvent.click(canvas.getByTestId('agents-a2ui-booking-confirm'))
    await expect(onaction).toHaveBeenCalledWith(expect.objectContaining({ name: 'confirm', context: { name: 'Ada' } }))
  }}
/>

<!-- A2UI 1.0, every component of the basic catalog: inputs write to the data, and the button's context reads it. -->
<Story
  name="The basic catalog"
  args={{ surface: CATALOG }}
  play={async ({ canvas, userEvent }) => {
    const surface = canvas.getByTestId('agents-surface-catalog')
    await expect(surface).toHaveTextContent('Drawn for Ada.')
    await expect(surface).toHaveTextContent('Grace')
    await expect(surface).toHaveTextContent('Linus')
    await expect(surface).toHaveTextContent('This window cannot draw “Hologram”.')
    await userEvent.click(segment(canvas, 'agents-a2ui-catalog-size', 'l'))
    await userEvent.click(canvas.getByTestId('agents-a2ui-catalog-agree'))
    await userEvent.click(canvas.getByTestId('agents-a2ui-catalog-go'))
    await expect(onaction).toHaveBeenCalledWith(expect.objectContaining({ name: 'go', context: { size: ['l'], agree: false } }))
    await userEvent.click(segment(canvas, 'agents-a2ui-catalog-tabs', '1'))
    await expect(surface).toHaveTextContent('The second tab.')
    await userEvent.click(canvas.getByTestId('agents-a2ui-catalog-more'))
    await expect(surface).toHaveTextContent('What was behind it.')
  }}
/>

<!-- The agent is busy: nothing on the surface can be pressed. -->
<Story
  name="While the agent is busy"
  args={{ disabled: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-a2ui-booking-confirm')).toHaveProperty('disabled', true)
  }}
/>

<!-- Created, and its components still to come. -->
<Story name="Nothing drawn yet" args={{ surface: { id: 'empty', components: new Map(), data: {} } }} />
