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
    const banner = within(canvasElement).getByTestId('banner')
    await expect(banner.getBoundingClientRect().height).toBeLessThan(200)
  }}
/>

<!-- Right to left: the title takes its own direction and does not reorder the app name beside it. -->
<Story name="Right to left" args={{ title: 'اكتمل النسخ الاحتياطي', body: 'تم نسخ ١٢ ملفًا.' }} />

<Story
  name="Select and dismiss"
  play={async ({ canvasElement }) => {
    select.mockClear()
    dismiss.mockClear()
    const canvas = within(canvasElement)
    // Touch targets: at least 44 points.
    for (const id of ['banner-open', 'banner-dismiss']) {
      const box = canvas.getByTestId(id).getBoundingClientRect()
      await expect(box.height).toBeGreaterThanOrEqual(44)
    }
    await expect(canvas.getByTestId('banner-dismiss').getBoundingClientRect().width).toBeGreaterThanOrEqual(44)
    await userEvent.click(canvas.getByTestId('banner-open'))
    await waitFor(() => expect(select).toHaveBeenCalledTimes(1))
    await userEvent.click(canvas.getByTestId('banner-dismiss'))
    await waitFor(() => expect(dismiss).toHaveBeenCalledTimes(1))
  }}
/>

<!-- By keyboard: Tab reaches the banner, Enter opens the app, Escape dismisses it. -->
<Story
  name="Keyboard"
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
  }}
/>

<!-- WCAG 2.2.1: the owner pauses the expiry while the pointer or focus is on the banner, and restarts it afterwards. -->
<Story
  name="Held while read"
  play={async ({ canvasElement }) => {
    hold.mockClear()
    release.mockClear()
    const canvas = within(canvasElement)
    const banner = canvas.getByTestId('banner')
    await userEvent.hover(banner)
    await expect(hold).toHaveBeenCalledTimes(1)
    await userEvent.unhover(banner)
    await expect(release).toHaveBeenCalledTimes(1)
    await userEvent.tab()
    await expect(hold).toHaveBeenCalledTimes(2)
    await userEvent.tab()
    await expect(canvas.getByTestId('banner-dismiss')).toHaveFocus()
    await expect(release).toHaveBeenCalledTimes(1)
    await userEvent.tab()
    await waitFor(() => expect(release).toHaveBeenCalledTimes(2))
  }}
/>
