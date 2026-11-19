<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor, within } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import ViewHost from '../organisms/ViewHost.svelte'
  import type { TreeApp } from '../host/treeApp'
  import { openAppStory, settleAtTop, until, type StoryHandle } from '../host/storyKit'
  import {
    galleryApp,
    keypadApp,
    listStyleApp,
    longListApp,
    reorderApp,
    splitApp,
    stackApp,
    swipeApp
  } from './kitTrees'

  /** A control's hit area, in CSS pixels: every touch target is at least 44 x 44. */
  const expectHitArea = async (el: HTMLElement): Promise<void> => {
    const box = el.getBoundingClientRect()
    await expect(box.width).toBeGreaterThanOrEqual(44)
    await expect(box.height).toBeGreaterThanOrEqual(44)
  }

  /** The scenario on screen, for its play function (the host side: events received, rows to change). */
  let latest: Promise<StoryHandle<TreeApp>> | null = null
  const story = async (): Promise<StoryHandle<TreeApp>> => {
    await until(() => latest !== null, 'the story session')
    return latest!
  }

  const SCENARIOS: Record<string, () => TreeApp> = {
    'style:plain': () => listStyleApp('plain'),
    'style:inset': () => listStyleApp('inset'),
    'style:insetGrouped': () => listStyleApp('insetGrouped'),
    'style:grouped': () => listStyleApp('grouped'),
    'style:sidebar': () => listStyleApp('sidebar'),
    long: () => longListApp(5000),
    refresh: () => longListApp(40, { refresh: true }),
    swipe: swipeApp,
    reorder: reorderApp,
    split3: () => splitApp(3),
    split2: () => splitApp(2),
    stack: stackApp,
    gallery: galleryApp,
    keypad: keypadApp
  }
  const open = (
    scenario: string,
    size: { w: number; h: number }
  ): Promise<StoryHandle<TreeApp>> => {
    void latest?.then((previous) => previous.dispose()) // a re-render replaces the session; do not leak the old one
    return (latest = openAppStory(SCENARIOS[scenario](), size))
  }

  /** The tap target of a managed `listitem` row (a click on the row itself reaches it by hit-testing, not here). */
  const tap = (id: string): string => `${id}-tap`

  /**
   * The tier-1 layout, navigation and collection components rendered through the registry from JSON view trees, on
   * the fake host. Each story is a window at its preferred size; the stories spec also opens it at the smallest
   * window (320x240) and checks nothing scrolls sideways.
   */
  const { Story } = defineMeta({
    title: 'Renderer/Kit — layout',
    component: ViewHost,
    tags: ['app'],
    args: { width: 480, height: 640, scenario: 'style:insetGrouped' },
    argTypes: { scenario: { control: false } },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number; scenario: string })}
  <AppWindow title="Demo" width={args.width} height={args.height} minWidth={320} minHeight={240}>
    {#await open(args.scenario, { w: args.width, h: args.height })}
      <p class="loading">Connecting…</p>
    {:then s}
      <div class="fill" {@attach () => () => s.dispose()}>
        <ViewHost view={s.view} app="demo" onretry={() => s.harness.session.retryNow()} />
      </div>
    {/await}
  </AppWindow>
{/snippet}

<!-- List styles: the same two sections in each of the five styles; a selected row; chevron, value and destructive rows. -->

<Story
  name="List, plain"
  args={{ scenario: 'style:plain' }}
  play={async ({ canvas, userEvent }) => {
    const list = await canvas.findByTestId('demo-list')
    await expect(list).toHaveAttribute('role', 'listbox') // a selection makes it a listbox of options
    const wifi = await canvas.findByTestId('demo-list-network-wifi')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    const vpn = canvas.getByRole('option', { name: /VPN/ })
    await userEvent.click(vpn)
    await waitFor(() => expect(vpn).toHaveAttribute('aria-selected', 'true'))
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
  }}
/>

<Story
  name="List, inset"
  args={{ scenario: 'style:inset' }}
  play={async ({ canvas, userEvent }) => {
    const list = await canvas.findByTestId('demo-list')
    await expect(list).toHaveAttribute('role', 'listbox') // a selection makes it a listbox of options
    const wifi = await canvas.findByTestId('demo-list-network-wifi')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    const vpn = canvas.getByRole('option', { name: /VPN/ })
    await userEvent.click(vpn)
    await waitFor(() => expect(vpn).toHaveAttribute('aria-selected', 'true'))
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
  }}
/>

<Story
  name="List, insetGrouped"
  args={{ scenario: 'style:insetGrouped' }}
  play={async ({ canvas, userEvent }) => {
    const list = await canvas.findByTestId('demo-list')
    await expect(list).toHaveAttribute('role', 'listbox') // a selection makes it a listbox of options
    const wifi = await canvas.findByTestId('demo-list-network-wifi')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    const vpn = canvas.getByRole('option', { name: /VPN/ })
    await userEvent.click(vpn)
    await waitFor(() => expect(vpn).toHaveAttribute('aria-selected', 'true'))
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
  }}
/>

<Story
  name="List, grouped"
  args={{ scenario: 'style:grouped' }}
  play={async ({ canvas, userEvent }) => {
    const list = await canvas.findByTestId('demo-list')
    await expect(list).toHaveAttribute('role', 'listbox') // a selection makes it a listbox of options
    const wifi = await canvas.findByTestId('demo-list-network-wifi')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    const vpn = canvas.getByRole('option', { name: /VPN/ })
    await userEvent.click(vpn)
    await waitFor(() => expect(vpn).toHaveAttribute('aria-selected', 'true'))
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
  }}
/>

<Story
  name="List, sidebar"
  args={{ scenario: 'style:sidebar' }}
  play={async ({ canvas, userEvent }) => {
    const list = await canvas.findByTestId('demo-list')
    await expect(list).toHaveAttribute('role', 'listbox') // a selection makes it a listbox of options
    const wifi = await canvas.findByTestId('demo-list-network-wifi')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    const vpn = canvas.getByRole('option', { name: /VPN/ })
    await userEvent.click(vpn)
    await waitFor(() => expect(vpn).toHaveAttribute('aria-selected', 'true'))
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
  }}
/>

<!-- Five thousand rows: a few dozen are in the DOM, the scrollbar is honest, the host sends windows, Home/End work. -->
<Story
  name="List, 5,000 rows (virtualized)"
  args={{ scenario: 'long' }}
  play={async ({ canvas, canvasElement, userEvent }) => {
    await canvas.findByText('Item 1')
    const rendered = (): number =>
      canvasElement.querySelectorAll('[data-row-key], [aria-busy=true][role=listitem]').length
    await waitFor(() => expect(rendered()).toBeGreaterThan(5))
    await expect(rendered()).toBeLessThan(60) // not 5,000
    const first = await canvas.findByTestId('demo-list-row-1-item')
    await expect(first).toHaveAttribute('aria-posinset', '1')
    await expect(first).toHaveAttribute('aria-setsize', '5000')
    const { harness } = await story()

    // Scroll far down: placeholders hold the geometry, then the host's window arrives.
    const scroller = canvasElement.querySelector<HTMLElement>('.scroll')!
    scroller.scrollTop = 120_000
    await canvas.findAllByText(/^Item 2[0-9]{3}$/, undefined, { timeout: 4000 })
    await waitFor(() =>
      expect(harness.host.received.some((r) => r.method === 'source.set')).toBe(true)
    )
    await expect(rendered()).toBeLessThan(60)

    // End jumps to the last row; Home back to the first.
    const some = canvasElement.querySelector<HTMLElement>('[data-row-focus]')!
    some.focus()
    await userEvent.keyboard('{End}')
    await waitFor(
      () => expect(canvasElement.querySelector('[data-row-key="5000"]')).toHaveFocus(),
      {
        timeout: 4000
      }
    )
    await userEvent.keyboard('{Home}')
    await waitFor(() => expect(canvasElement.querySelector('[data-row-key="1"]')).toHaveFocus(), {
      timeout: 4000
    })
    await settleAtTop(scroller, canvasElement)
    await expect(scroller.scrollTop).toBe(0)
  }}
/>

<!-- Swipe actions: reachable from the keyboard (Alt+Left opens the trailing ones); Delete removes the row. -->
<Story
  name="Swipe actions"
  args={{ scenario: 'swipe' }}
  play={async ({ canvas, canvasElement, userEvent }) => {
    const first = await canvas.findByTestId(tap('demo-list-row-1-item'))
    first.focus()
    await userEvent.keyboard('{Alt>}{ArrowLeft}{/Alt}')
    const del = await canvas.findByTestId('demo-list-row-1-item-delete')
    await waitFor(() => expect(del).toHaveFocus())
    await expect(canvasElement.querySelector('[data-row-key="1"]')).toHaveAttribute(
      'data-open',
      'trailing'
    )
    await userEvent.click(del)
    await waitFor(() => expect(canvas.queryByText('Groceries')).toBeNull())
    await expect(canvas.getByText('Ideas')).toBeVisible()
    // The other rows renumber: five became four.
    await waitFor(() =>
      expect(canvas.getByTestId('demo-list-row-2-item')).toHaveAttribute('aria-setsize', '4')
    )
  }}
/>

<!-- Reorder: Alt+ArrowDown moves the focused row; the app receives onMove {key, beforeKey} and the new order sticks. -->
<Story
  name="Reorder"
  args={{ scenario: 'reorder', height: 520 }}
  play={async ({ canvas, userEvent }) => {
    const items = (): string[] =>
      canvas
        .getAllByRole('listitem')
        .map((el) => el.textContent?.replace(/Reorder.*$/, '').trim() ?? '')
    const milk = await canvas.findByTestId('demo-list-row-1-item-face')
    await waitFor(() => expect(items()[0]).toContain('Milk'))
    milk.focus()
    await userEvent.keyboard('{Alt>}{ArrowDown}{/Alt}')
    const { harness } = await story()
    await waitFor(() => expect(harness.app.eventsOf('move')).toHaveLength(1))
    await expect(harness.app.eventsOf('move')[0].payload).toEqual({ key: 1, beforeKey: 3 })
    await waitFor(() => expect(items()[0]).toContain('Eggs'))
    await waitFor(() => expect(items()[1]).toContain('Milk'))
    await expect(await canvas.findByText('Milk moved to position 2 of 6')).toBeInTheDocument()
    // Drag handles are there for the pointer (edit mode is on).
    await expect(canvas.getByTestId('demo-list-row-3-item-handle')).toBeVisible()
  }}
/>

<!-- Pull to refresh: the keyboard "Refresh" button runs onRefresh; the list is busy until the app answers. -->
<Story
  name="Pull to refresh"
  args={{ scenario: 'refresh' }}
  play={async ({ canvas, userEvent }) => {
    const refresh = await canvas.findByTestId('demo-list-refresh')
    refresh.focus()
    await expect(refresh).toBeVisible() // visible while focused
    await userEvent.click(refresh)
    const { harness } = await story()
    await waitFor(() => expect(harness.app.eventsOf('refresh')).toHaveLength(1))
    await expect(await canvas.findByTestId('demo-list')).toBeVisible()
    await waitFor(() =>
      expect(canvas.getByTestId('demo-list').parentElement).not.toHaveAttribute('aria-busy')
    )
  }}
/>

<!-- Three columns: folders (a navigation landmark) beside notes beside the note; picking a folder is client state. -->
<Story
  name="Split view, three columns"
  args={{ width: 1000, height: 560, scenario: 'split3' }}
  play={async ({ canvas, userEvent }) => {
    // The stories spec also opens this at the smallest window, where the split is collapsed to one column.
    const wide = canvas.getByTestId('app-window').clientWidth >= 700
    if (!wide) {
      await expect(await canvas.findByRole('option', { name: /Notes/ })).toBeVisible()
      return
    }
    const nav = await canvas.findByRole('navigation')
    await expect(within(nav).getByRole('option', { name: /Notes/ })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    await expect(canvas.getByRole('main')).toBeVisible()
    await userEvent.click(within(nav).getByRole('option', { name: /Reminders/ }))
    await waitFor(() =>
      expect(within(nav).getByRole('option', { name: /Reminders/ })).toHaveAttribute(
        'aria-selected',
        'true'
      )
    )
    await expect(await canvas.findByRole('heading', { name: 'REMINDERS' })).toBeVisible()
    await userEvent.click(canvas.getByRole('option', { name: /Trip plan/ }))
    await waitFor(() => expect(canvas.getByText('id 3')).toBeVisible())
  }}
/>

<!-- Two columns: sidebar and detail (a 560 px window: still two columns). -->
<Story
  name="Split view, two columns"
  args={{ width: 760, height: 480, scenario: 'split2' }}
  play={async ({ canvas }) => {
    await expect(await canvas.findByRole('navigation')).toBeVisible()
    // At the smallest window the split is one column: the sidebar alone.
    if (canvas.getByTestId('app-window').clientWidth >= 700)
      await expect(canvas.getByRole('main')).toBeVisible()
  }}
/>

<!-- Narrow window: the split collapses to a navigation flow. Choose a folder, then a note; back goes back. -->
<Story
  name="Split view, collapsed"
  args={{ width: 480, height: 560, scenario: 'split3' }}
  play={async ({ canvas, userEvent }) => {
    // The stories spec also opens this at 1024 px, where nothing collapses: both columns are simply there.
    if (canvas.getByTestId('app-window').clientWidth >= 700) {
      await expect(await canvas.findByRole('navigation')).toBeVisible()
      await expect(canvas.getByRole('main')).toBeVisible()
      return
    }
    const folder = await canvas.findByRole('option', { name: /Notes/ })
    await waitFor(() => expect(canvas.queryByRole('main')).toBeNull()) // only the sidebar is showing
    await userEvent.click(folder)
    const note = await canvas.findByRole('option', { name: /Groceries/ })
    await userEvent.click(note)
    await waitFor(async () => expect(await canvas.findByText('id 1')).toBeVisible())
    // The detail's back button is named after the screen it returns to.
    const back = await canvas.findByRole('button', { name: /NOTES/ })
    await userEvent.click(back)
    await expect(await canvas.findByRole('option', { name: /Ideas/ })).toBeVisible()
    await userEvent.click(await canvas.findByRole('button', { name: /Folders/ }))
    await expect(await canvas.findByRole('option', { name: /Reminders/ })).toBeVisible()
  }}
/>

<!-- Navigation stack: a link pushes a route, the back button is named after the previous screen, focus moves. -->
<Story
  name="Navigation stack, push and pop"
  args={{ scenario: 'stack' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId(tap('demo-inbox-list-row-2-link')))
    await expect(await canvas.findByRole('heading', { name: 'Ideas' })).toBeVisible()
    await expect(await canvas.findByText('Note id 2')).toBeVisible()
    const back = await canvas.findByRole('button', { name: 'Inbox' })
    await expectHitArea(back)
    // Push again from inside the screen (a nav.push action), then all the way back with popToRoot.
    await userEvent.click(await canvas.findByRole('button', { name: 'Open related note' }))
    await expect(await canvas.findByRole('heading', { name: 'Related' })).toBeVisible()
    await expect(await canvas.findByRole('button', { name: 'Ideas' })).toBeVisible()
    await userEvent.click(await canvas.findByRole('button', { name: 'Back to Inbox' }))
    await waitFor(() => expect(canvas.queryByText('Note id 99')).toBeNull(), { timeout: 3000 })
    await expect(await canvas.findByRole('heading', { name: 'Inbox' })).toBeVisible()
    void back
  }}
/>

<!-- The rest of the layout kit: tabs, grid, z-stack, spacer, divider, adaptive, scope, card, form, disclosure, empty state. -->
<Story
  name="Layout gallery"
  args={{ scenario: 'gallery', width: 520, height: 720 }}
  play={async ({ canvas, userEvent }) => {
    await expect(await canvas.findByText('Spans two columns')).toBeVisible()
    await expect(canvas.getByText(/(Regular|Compact) layout/)).toBeVisible()
    await expect(canvas.getByText('A dark scope inside a light window')).toBeVisible()
    await userEvent.click(canvas.getByRole('tab', { name: 'Form' }))
    await expect(await canvas.findByRole('form', { name: 'Preferences' })).toBeVisible()
    const more = await canvas.findByRole('button', { name: 'More' })
    await expect(more).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(more)
    await waitFor(() => expect(more).toHaveAttribute('aria-expanded', 'true'))
    await expect(await canvas.findByText('a1b2c3')).toBeVisible()
    await userEvent.click(canvas.getByRole('tab', { name: /Empty/ }))
    await expect(await canvas.findByText('No messages')).toBeVisible()
    await expect(canvas.getByRole('button', { name: 'Compose' })).toBeVisible()
  }}
/>

<!-- Fill buttons and a spanning cell: circular keys that fill their square cells, a "0" of two columns and the gap, and a display aligned to the keypad. -->
<Story
  name="Keypad, fill buttons and a spanning cell"
  args={{ scenario: 'keypad', width: 340, height: 300 }}
  play={async ({ canvas }) => {
    const key = (name: string): HTMLElement => canvas.getByRole('button', { name })
    const box = (name: string): DOMRect => key(name).getBoundingClientRect()
    const seven = box('7')
    const gap = box('8').left - seven.right
    for (const name of ['7', '9', '÷', '.', '=']) {
      const b = box(name)
      await expect(Math.abs(b.width - b.height)).toBeLessThan(1) // a circle, not a pill
      await expect(Math.abs(b.width - seven.width)).toBeLessThan(1)
      await expect(parseFloat(getComputedStyle(key(name)).borderRadius)).toBeGreaterThanOrEqual(
        b.height / 2
      )
    }
    const zero = box('0')
    await expect(Math.abs(zero.width - (2 * seven.width + gap))).toBeLessThan(1) // two columns and the gap between
    await expect(Math.abs(zero.height - seven.height)).toBeLessThan(1) // the row keeps its height
    await expect(Math.abs(zero.left - box('4').left)).toBeLessThan(1)
    const label = canvas.getByTestId('keypad-zero-label')
    await expect(label.getBoundingClientRect().left - zero.left).toBeLessThan(zero.height / 2) // over the first circle, not centred
    const display = canvas.getByTestId('keypad-display').getBoundingClientRect()
    await expect(Math.abs(display.right - box('=').right)).toBeLessThan(1)
    await expect(Math.abs(display.left - seven.left)).toBeLessThan(1)
  }}
/>

<style>
  .fill {
    height: 100%;
  }
  .loading {
    padding: 16px;
  }
</style>
