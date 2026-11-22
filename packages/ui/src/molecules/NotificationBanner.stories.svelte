<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import NotificationBanner from './NotificationBanner.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/NotificationBanner',
    component: NotificationBanner,
    tags: ['autodocs'],
    args: {
      testid: 'banner',
      appName: 'Backups',
      icon: 'clock',
      background: 'linear-gradient(180deg, #5ac8fa, #007aff)',
      title: 'Backup finished',
      body: '12 files copied to the NAS.'
    },
    parameters: { controls: { disable: true } },
    render: template
  })

  const select = fn()
  const dismiss = fn()
  const hold = fn()
  const release = fn()

  const long =
    'The nightly backup of the photo library, the documents folder and every project folder finished with warnings'
  const longBody = Array.from({ length: 8 }, (_, i) => `Line ${i + 1}: a file was skipped because it was in use.`).join('\n')
</script>

{#snippet template(args: {
  testid: string
  appName: string
  icon: 'clock'
  background: string
  title: string
  body?: string
})}
  <div style="width:380px">
    <NotificationBanner {...args} onselect={select} ondismiss={dismiss} onhold={hold} onrelease={release} />
  </div>
{/snippet}

<Story name="One alert" />

<Story name="Title only" args={{ body: undefined }} />

<!-- One line for the title (cut with an ellipsis) and four for the body; the card does not grow past that. -->
<Story
  name="Long text"
  args={{ title: long, body: longBody }}
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByTestId('banner').getBoundingClientRect().height).toBeLessThan(200)
    // the control is named by the sender and the title; the body is its description, not part of its name
    const open = canvas.getByTestId('banner-open')
    await expect(open).toHaveAccessibleName(`Open Backups: ${long}`)
    await expect(open).toHaveAccessibleDescription(/^Line 1: a file was skipped/)
  }}
/>

<!-- Right to left: the title takes its own direction and does not reorder the app name beside it. -->
<Story name="Right to left" args={{ title: 'اكتمل النسخ الاحتياطي', body: 'تم نسخ ١٢ ملفًا.' }} />

<!-- Behaviour only: the end state is the one alert of the first story. -->
<Story
  name="Select and dismiss"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    select.mockClear()
    dismiss.mockClear()
    const canvas = within(canvasElement)
    // Touch targets: at least 44 points. The control that opens the app reaches over the whole card; Dismiss is 44 x 44.
    await expect(canvas.getByTestId('banner').getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
    const dismissBox = canvas.getByTestId('banner-dismiss').getBoundingClientRect()
    await expect(dismissBox.width).toBeGreaterThanOrEqual(44)
    await expect(dismissBox.height).toBeGreaterThanOrEqual(44)
    // a click on the body, away from the title, lands on the same control
    const card = canvas.getByTestId('banner').getBoundingClientRect()
    await expect(document.elementFromPoint(card.left + 60, card.bottom - 10)).toBe(canvas.getByTestId('banner-open'))
    await userEvent.click(canvas.getByTestId('banner-open'))
    await waitFor(() => expect(select).toHaveBeenCalledTimes(1))
    await userEvent.click(canvas.getByTestId('banner-dismiss'))
    await waitFor(() => expect(dismiss).toHaveBeenCalledTimes(1))
  }}
/>

<!-- By keyboard: Tab reaches the banner, Enter opens the app, Escape dismisses it. It ends with the keys on the
     banner (the ring is the state this one's baseline shows). -->
<Story
  name="Keyboard"
  tags={['keep-focus']}
  play={async ({ canvasElement }) => {
    select.mockClear()
    dismiss.mockClear()
    const canvas = within(canvasElement)
    await userEvent.tab()
    await expect(canvas.getByTestId('banner-open')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(select).toHaveBeenCalledTimes(1))
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(dismiss).toHaveBeenCalledTimes(1))
    await userEvent.tab()
    await expect(canvas.getByTestId('banner-dismiss')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(dismiss).toHaveBeenCalledTimes(2))
    // left as a keyboard user leaves it: on the control that opens the app, with its focus ring
    const options = { focusVisible: true } as unknown as Parameters<HTMLElement['focus']>[0]
    canvas.getByTestId('banner-open').focus(options)
    await expect(canvas.getByTestId('banner-open')).toHaveFocus()
  }}
/>

<!-- WCAG 2.2.1: the keyboard on the banner, or a pointer that is being moved over it, holds its expiry; a pointer that
     merely rests where the banner appeared does not (or it would stay for as long as the mouse does). -->
<Story
  name="Held while read"
  tags={['play-only']}
  play={async ({ canvasElement }) => {
    hold.mockClear()
    release.mockClear()
    const canvas = within(canvasElement)
    const banner = canvas.getByTestId('banner')
    const pointer = (type: string, movementX: number): void => {
      banner.dispatchEvent(new PointerEvent(type, { bubbles: true, movementX, movementY: 0 }))
    }
    // resting: the pointer is over the banner, and the browser repeats its position, but it has not moved
    pointer('pointerenter', 0)
    pointer('pointermove', 0)
    await expect(hold).not.toHaveBeenCalled()
    // in use: it is held, and let go when the pointer leaves
    pointer('pointermove', 4)
    pointer('pointermove', 2)
    await expect(hold).toHaveBeenCalledTimes(1)
    pointer('pointerleave', 0)
    await expect(release).toHaveBeenCalledTimes(1)
    // the keyboard on it holds it too, and moving between its own buttons does not release it
    await userEvent.tab()
    await expect(hold).toHaveBeenCalledTimes(2)
    await userEvent.tab()
    await expect(canvas.getByTestId('banner-dismiss')).toHaveFocus()
    await expect(release).toHaveBeenCalledTimes(1)
    await userEvent.tab()
    await waitFor(() => expect(release).toHaveBeenCalledTimes(2))
  }}
/>
