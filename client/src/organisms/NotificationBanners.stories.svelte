<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
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

  // Nothing expires while a story is looked at: the clock never fires.
  const center = new Notifications({ wm, appOf, schedule: () => () => {} })

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
  const long =
    'The nightly backup of the photo library, the documents folder and every project folder finished with warnings'
  const longBody = Array.from(
    { length: 10 },
    (_, i) => `Line ${i + 1}: a file was skipped because it was in use.`
  ).join('\n')
</script>

{#snippet template()}
  <div style="position:relative;height:420px;background:var(--bg-grouped)">
    <button type="button" data-testid="story-before" style="margin:8px"
      >Something with the keys</button
    >
    <NotificationBanners {center} />
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
    center.deliver({ app: 'agents', title: 'Second', body: 'From another app' })
    calc('Third', 'The newest is on top')
    calc('Fourth', 'A fourth takes the place of the oldest')
    const canvas = within(canvasElement)
    await waitFor(() => expect(canvas.getAllByTestId(/^notification-\d+$/)).toHaveLength(3))
    const titles = canvas.getAllByTestId(/^notification-\d+$/).map((b) => b.textContent ?? '')
    await expect(titles[0]).toContain('Fourth')
    await expect(titles[2]).toContain('Second')
  }}
/>

<!-- One line for the title, a few for the body: the card does not grow with the text. -->
<Story
  name="Long text"
  play={async ({ canvasElement }) => {
    calc(long, longBody)
    const banner = await within(canvasElement).findByTestId(/^notification-\d+$/)
    await expect(banner.getBoundingClientRect().height).toBeLessThan(200)
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

<Story
  name="Title with nothing visible shows the app"
  play={async ({ canvasElement }) => {
    calc(ZWSP.repeat(3))
    const banner = await within(canvasElement).findByTestId(/^notification-\d+$/)
    await expect(banner.querySelector('.title')).toHaveTextContent('Calculator')
  }}
/>

<!-- The sender has the window in front on this client: nothing is shown here. -->
<Story
  name="Sender in front shows nothing"
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
  play={async ({ canvasElement }) => {
    calc('One')
    calc('Two')
    const canvas = within(canvasElement)
    await waitFor(() => expect(canvas.getAllByTestId(/^notification-\d+$/)).toHaveLength(2))
    await userEvent.click(canvas.getAllByTestId(/-dismiss$/)[0]!)
    await waitFor(() => expect(canvas.getAllByTestId(/^notification-\d+$/)).toHaveLength(1))
    await expect(open).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId(/-open$/))
    await waitFor(() => expect(canvas.queryByTestId('notifications')).toBeNull())
    await expect(open).toHaveBeenCalledWith('calculator', { kind: 'main' })
  }}
/>

<!-- By keyboard: Alt+Shift+N goes to the newest banner, Escape dismisses it and gives focus back. -->
<Story
  name="Keyboard"
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
  }}
/>
