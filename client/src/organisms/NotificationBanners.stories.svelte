<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import { Text } from '@vos/ui/atoms'
  import { Window } from '@vos/ui/organisms'
  import NotificationBanners from './NotificationBanners.svelte'
  import { appOf, setInstalledApps } from '../lib/apps.svelte'
  import { Notifications } from '../lib/notifications.svelte'
  import { FAKE_AGENTS_APP, FAKE_CALCULATOR } from '../shell/fakeHost'

  /** The window manager as the alerts see it: which window is in front, and a spy for "open". */
  const open = fn()
  let front: string | undefined
  const wm = {
    get focused() {
      return front ? ({ appId: front, layout: 'floating', stageId: 's' } as never) : undefined
    },
    activeStageId: 's',
    slideOverHidden: false,
    open
  }

  // Nothing expires while a story is looked at: the clock never fires. The page is in use and on screen.
  const center = new Notifications({
    bannersOn: () => true,
    wm,
    appOf,
    schedule: () => () => {},
    pageVisible: () => true,
    pageInFront: () => true,
    onPageVisibility: () => () => {}
  })

  const { Story } = defineMeta({
    title: 'Desktop/NotificationBanners',
    component: NotificationBanners,
    tags: ['autodocs'],
    parameters: { layout: 'fullscreen', controls: { disable: true } },
    beforeEach: () => {
      setInstalledApps([FAKE_CALCULATOR, FAKE_AGENTS_APP])
      center.banners = []
      front = undefined
      open.mockClear()
    },
    render: template
  })

  // the characters that cannot be written into this file as they are (Svelte warns on a bidirectional control)
  const RLO = String.fromCharCode(0x202e)
  const ZWSP = String.fromCharCode(0x200b)
  const calc = (title: string, body?: string): void =>
    center.deliver({ app: 'calculator', title, ...(body === undefined ? {} : { body }) })
  const agents = (title: string, body?: string): void =>
    center.deliver({ app: 'agents', title, ...(body === undefined ? {} : { body }) })
  const long =
    'The nightly backup of the photo library, the documents folder and every project folder finished with warnings'
  const longBody = Array.from(
    { length: 10 },
    (_, i) => `Line ${i + 1}: a file was skipped because it was in use.`
  ).join('\n')
  const rows = Array.from(
    { length: 12 },
    (_, i) => `Project notes ${i + 1}: a line of what is in the window`
  )
  const banners = (canvas: ReturnType<typeof within>): HTMLElement[] =>
    canvas.queryAllByTestId(/^notification-\d+$/)
  /** Focus the way a keyboard user's arrives: with the ring. */
  const ring = (el: HTMLElement): void =>
    el.focus({ focusVisible: true } as unknown as Parameters<HTMLElement['focus']>[0])
</script>

{#snippet template()}
  <div style="position:relative;height:460px;background:var(--bg-grouped)">
    <button type="button" data-testid="story-before" style="margin:8px"
      >Something with the keys</button
    >
    <!-- a busy window behind the banners: they are drawn over an app, not over a flat page -->
    <Window
      testid="window-files"
      title="Files"
      x={140}
      y={70}
      width={640}
      height={360}
      focused={false}
    >
      <div style="padding:16px;display:grid;gap:6px">
        <Text style="title2">Files</Text>
        {#each rows as row (row)}<Text>{row}</Text>{/each}
      </div>
    </Window>
    <NotificationBanners
      {center}
      fallback={() => document.querySelector<HTMLElement>('[data-testid="story-before"]')}
    />
  </div>
{/snippet}

<!-- The sender's name and icon are the shell's own (the Calculator's), whatever the app wrote. -->
<Story
  name="One alert"
  play={async ({ canvasElement }) => {
    calc('Backup finished', '12 files copied to the NAS.')
    const canvas = within(canvasElement)
    const banner = await canvas.findByTestId(/^notification-\d+$/)
    await expect(banner).toHaveTextContent('Calculator')
    await expect(banner).toHaveTextContent('Backup finished')
    // announced from a region that is always there
    await expect(canvas.getByTestId('notifications-live')).toHaveTextContent(
      'Calculator: Backup finished. 12 files copied to the NAS.'
    )
  }}
/>

<Story
  name="Three stacked"
  play={async ({ canvasElement }) => {
    calc('First')
    agents('Second', 'From another app')
    calc('Third', 'The newest is on top')
    agents('Fourth', 'A fourth takes the place of the sender’s own oldest')
    const canvas = within(canvasElement)
    await waitFor(() => expect(banners(canvas)).toHaveLength(3))
    const titles = banners(canvas).map((b) => b.textContent ?? '')
    await expect(titles[0]).toContain('Fourth')
    await expect(titles[2]).toContain('First')
  }}
/>

<!-- One app's burst competes with itself: the quiet app's one alert is still on the screen. -->
<Story
  name="A burst does not push another app's alert off"
  play={async ({ canvasElement }) => {
    agents('One from the quiet app', 'It is still here')
    calc('Burst 1')
    calc('Burst 2')
    calc('Burst 3')
    const canvas = within(canvasElement)
    await waitFor(() => expect(banners(canvas)).toHaveLength(3))
    const texts = banners(canvas).map((b) => b.textContent ?? '')
    await expect(texts.some((t) => t.includes('One from the quiet app'))).toBe(true)
    await expect(texts.some((t) => t.includes('Burst 1'))).toBe(false)
  }}
/>

<!-- One line for the title, a few for the body: the card does not grow with the text, and the control's name is not the body. -->
<Story
  name="Long text"
  play={async ({ canvasElement }) => {
    calc(long, longBody.repeat(20).slice(0, 2000))
    const canvas = within(canvasElement)
    const banner = await canvas.findByTestId(/^notification-\d+$/)
    await expect(banner.getBoundingClientRect().height).toBeLessThan(200)
    // what the live region says and what the control is described by is the start of the body, not two thousand characters
    const said = canvas.getByTestId('notifications-live').textContent ?? ''
    await expect(said.length).toBeLessThan(500) // the sender, a 110-character title and 300 of the body
    const control = canvas.getByTestId(/-open$/)
    await expect(control).toHaveAccessibleName(`Open Calculator: ${long}`)
    await expect((control.getAttribute('aria-describedby') ?? '').length).toBeGreaterThan(0)
  }}
/>

<!-- A line break, an override and a zero-width space in the title, markup in the body: one clean line, and text. -->
<Story
  name="Untrusted text"
  play={async ({ canvasElement }) => {
    calc(`Odd\ntitle ${RLO}gnp.exe${ZWSP}`, 'first line\nsecond line <b>not bold</b>')
    const canvas = within(canvasElement)
    const banner = await canvas.findByTestId(/^notification-\d+$/)
    await expect(banner).toHaveTextContent('Odd title gnp.exe')
    await expect(banner).toHaveTextContent('second line <b>not bold</b>')
    await expect(banner.querySelector('b')).toBeNull()
  }}
/>

<!-- Text that joins: a family emoji, the flag of England, a Persian word with its non-joiner, an Indic conjunct. -->
<Story
  name="Text that joins"
  play={async ({ canvasElement }) => {
    const title =
      '\u{1F468}\u200D\u{1F469}\u200D\u{1F467}\u200D\u{1F466} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F} می\u200Cخواهم क्\u200Dष'
    calc(title, title)
    const canvas = within(canvasElement)
    const banner = await canvas.findByTestId(/^notification-\d+$/)
    await expect(banner.querySelector('.title')?.textContent).toBe(title)
  }}
/>

<Story
  name="Title with nothing visible shows the app"
  play={async ({ canvasElement }) => {
    calc(ZWSP.repeat(3))
    const banner = await within(canvasElement).findByTestId(/^notification-\d+$/)
    await expect(banner.querySelector('.title')).toHaveTextContent('Calculator')
  }}
/>

<!-- The sender has the window in front on this client: nothing is shown here (its end state is an empty desk). -->
<Story
  name="Sender in front shows nothing"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    front = 'calculator'
    calc('Hidden')
    calc('Also hidden')
    await expect(within(canvasElement).queryByTestId('notifications')).toBeNull()
    await expect(within(canvasElement).getByTestId('notifications-live')).toHaveTextContent('')
  }}
/>

<Story
  name="Select opens the sender, dismiss opens nothing"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    calc('One')
    calc('Two')
    const canvas = within(canvasElement)
    await waitFor(() => expect(banners(canvas)).toHaveLength(2))
    await userEvent.click(canvas.getAllByTestId(/-dismiss$/)[0]!)
    await waitFor(() => expect(banners(canvas)).toHaveLength(1))
    await expect(open).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId(/-open$/))
    await waitFor(() => expect(canvas.queryByTestId('notifications')).toBeNull())
    await expect(open).toHaveBeenCalledWith('calculator')
  }}
/>

<!-- Focus is never dropped to the page: it moves to the nearest banner, else back where it was, else to the fallback. -->
<Story
  name="Focus is not dropped when its banner goes"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const keys = (): string => {
      const el = document.activeElement
      return el && el !== document.body
        ? (el.getAttribute('data-testid') ?? el.tagName)
        : 'the page'
    }
    const kept = (step: string): void => {
      if (keys() === 'the page') throw new Error(`${step}: focus was dropped to the page`)
    }
    // 1. the focused banner is pushed out by three newer ones: focus is on a banner that is left
    calc('One')
    await canvas.findByTestId(/^notification-\d+$/)
    ring(canvas.getByTestId(/-open$/))
    agents('Two')
    agents('Three')
    agents('Four')
    await waitFor(() => expect(banners(canvas)).toHaveLength(3))
    kept('pushed out by newer ones')
    // 2. dismissed with the mouse, arrived at by Tab or a click (nothing to give back): the nearest banner, then the fallback
    ring(canvas.getAllByTestId(/-open$/)[0]!)
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(banners(canvas)).toHaveLength(2))
    kept('Escape')
    await userEvent.click(canvas.getAllByTestId(/-dismiss$/)[0]!)
    await userEvent.click(canvas.getAllByTestId(/-dismiss$/)[0]!)
    await waitFor(() => expect(canvas.queryByTestId('notifications')).toBeNull())
    await expect(canvas.getByTestId('story-before')).toHaveFocus()
    // 3. the focused banner's app stops running
    calc('Five')
    agents('Six')
    await waitFor(() => expect(banners(canvas)).toHaveLength(2))
    ring(canvas.getAllByTestId(/-open$/)[1]!) // the Calculator's, the older
    center.forgetApps(['calculator'])
    await waitFor(() => expect(banners(canvas)).toHaveLength(1))
    kept('its app stopped')
    await expect(canvas.getByTestId(/-open$/)).toHaveFocus()
  }}
/>

<!-- Typing is not intercepted: Option+Shift+N is a character on a Mac keyboard, so the shortcut leaves a field that takes
     typing alone (its event is not cancelled and the keys stay where they were), and works from anywhere else. -->
<Story
  name="The shortcut leaves typing alone"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    calc('Reminder', 'Stand up')
    const canvas = within(canvasElement)
    await canvas.findByTestId(/^notification-\d+$/)
    // what a Mac sends for Option+Shift+N: the physical key is N, the character is a tilde
    const macKey = (target: Element): KeyboardEvent => {
      const event = new KeyboardEvent('keydown', {
        code: 'KeyN',
        key: String.fromCharCode(0x2dc),
        altKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true
      })
      target.dispatchEvent(event)
      return event
    }
    const field = (tag: string, attributes: Record<string, string> = {}): HTMLElement => {
      const el = document.createElement(tag)
      for (const [name, value] of Object.entries(attributes)) el.setAttribute(name, value)
      canvasElement.append(el)
      return el
    }
    const editable = field('div', { contenteditable: 'true', tabindex: '0' })
    const inside = document.createElement('span')
    editable.append(inside)
    const typed: [string, HTMLElement][] = [
      ['a text input', field('input', { type: 'text' })],
      ['a search field', field('input', { type: 'search' })],
      ['a text area', field('textarea')],
      ['a select', field('select')],
      ['editable content', editable],
      ['inside editable content', inside]
    ]
    for (const [name, el] of typed) {
      el.focus()
      const event = macKey(el)
      await expect(event.defaultPrevented, `${name}: the key is left to type`).toBe(false)
      await expect(canvas.queryByTestId(/-open$/)).not.toHaveFocus()
    }
    // a control that takes no typing, and the page: the shortcut works
    const box = field('input', { type: 'checkbox' })
    box.focus()
    await expect(macKey(box).defaultPrevented).toBe(true)
    await waitFor(() => expect(canvas.getByTestId(/-open$/)).toHaveFocus())
    for (const [, el] of typed) el.remove()
    box.remove()
  }}
/>

<!-- By keyboard: Alt+Shift+N goes to the newest banner, Escape dismisses it and gives focus back. It ends with the keys
     on a banner, over a busy window: the focus ring is the state. -->
<Story
  name="Keyboard"
  tags={['keep-focus']}
  play={async ({ canvasElement }) => {
    calc('Reminder', 'Stand up')
    const canvas = within(canvasElement)
    await canvas.findByTestId(/^notification-\d+$/)
    const before = canvas.getByTestId('story-before')
    before.focus()
    await userEvent.keyboard('{Alt>}{Shift>}n{/Shift}{/Alt}')
    await waitFor(() => expect(canvas.getByTestId(/-open$/)).toHaveFocus())
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('notifications')).toBeNull())
    await waitFor(() => expect(before).toHaveFocus())
    await expect(open).not.toHaveBeenCalled()
    // and again, left on the banner for the picture
    calc('Reminder', 'Stand up')
    await canvas.findByTestId(/^notification-\d+$/)
    await userEvent.keyboard('{Alt>}{Shift>}n{/Shift}{/Alt}')
    await waitFor(() => expect(canvas.getByTestId(/-open$/)).toHaveFocus())
  }}
/>
