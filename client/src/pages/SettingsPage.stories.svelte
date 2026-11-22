<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fireEvent, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import SettingsPage from './SettingsPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { devices } from '../lib/devices.svelte'
  import { host } from '../lib/host.svelte'
  import { notifications } from '../lib/notifications.svelte'
  import { DEFAULTS, Settings, settings, STORAGE_KEY } from '../lib/settings.svelte'
  import { HostError } from '../shell/channel'
  import { createFakeHost, FAKE_CALCULATOR } from '../shell/fakeHost'
  import type { HostApi } from '../shell/hostApi'
  import type { Session } from '../shell/session'

  const app = apps.settings

  /**
   * The host a story talks to: three paired devices (this one first), or `devices` of them, and a host that refuses
   * the methods in `refuses` (each as many times as it says, then it answers).
   */
  const withHost =
    (options: { devices?: number; refuses?: Record<string, number> } = {}) =>
    (): void => {
      const fake = createFakeHost()
      if (options.devices !== undefined) fake.devices = fake.devices.slice(0, options.devices)
      const left = { ...options.refuses }
      const api: HostApi = {
        onNotification: (listener) => fake.onNotification(listener),
        call: (method: string, params: Record<string, unknown> = {}): Promise<unknown> => {
          if ((left[method] ?? 0) > 0) {
            left[method]!--
            return Promise.reject(new HostError(-32000, 'Not connected to vos'))
          }
          return fake.call(method, params)
        }
      }
      host.useFake(api, [FAKE_CALCULATOR])
    }

  /**
   * Every story starts as a browser that has chosen nothing, on a session whose host said its version, and leaves
   * nothing behind for the next one (the settings are stored, and the page is loaded again for each theme and size).
   */
  function fresh(): () => void {
    const reset = (): void => {
      Object.assign(settings, DEFAULTS)
      localStorage.removeItem(STORAGE_KEY)
      notifications.clear()
      Object.assign(devices, { list: [], loaded: false, error: null, asked: null })
    }
    reset()
    withHost()()
    host.session = { host: { hostVersion: '0.1.0' } } as unknown as Session
    return reset
  }

  /**
   * What `App.svelte` does for the shell: the settings are applied as they change, so a story shows what a control
   * does and its play function can measure it. The tint, the text size and the transparency go on the document, as
   * in the shell. The theme goes on the story's own frame instead: Storybook owns the document's (its `theme` global
   * stands for the system's here, and its decorator writes it again after every play function), so `auto` is
   * Storybook's theme, and Light or Dark shows on the window whatever the page around it is.
   */
  const applied =
    (storybookTheme: 'light' | 'dark') =>
    (frame: HTMLElement): (() => void) => {
      const root = document.documentElement
      settings.apply(frame)
      settings.apply(root)
      root.dataset.theme = storybookTheme
      return () => {
        root.style.removeProperty('--tint')
        root.style.removeProperty('--text-scale')
        root.classList.remove('reduce-transparency')
      }
    }

  const stored = (): unknown => JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null')
  const fontSizeOf = (el: HTMLElement): number => parseFloat(getComputedStyle(el).fontSize)

  /**
   * Settings: General (banners, the host's version), Appearance (theme, accent, text size, transparency) and Devices
   * (the list, revoking one, and what is said when the host cannot be asked). Each play function uses the control and
   * checks what it did: the store, what is kept for the next load, and the document.
   */
  const { Story } = defineMeta({
    title: 'Apps/Settings',
    component: SettingsPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    beforeEach: fresh,
    render: template
  })
</script>

{#snippet template(
  args: { width: number; height: number },
  context: { globals: Record<string, unknown> }
)}
  <div {@attach applied(context.globals.theme === 'Dark' ? 'dark' : 'light')}>
    <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
      <SettingsPage />
    </AppWindow>
  </div>
{/snippet}

<!-- General: banners are on, and the About row is the version the host said when the session opened. -->
<Story
  name="General"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-general'))
    await expect(canvas.getByTestId('settings-banners')).toHaveAttribute('aria-checked', 'true')
    await expect(canvas.getByTestId('settings-about-host')).toHaveTextContent('Host Version 0.1.0')
    // nothing fake is left: no account, no search, no Notifications or Privacy pane
    for (const gone of ['settings-account', 'settings-search', 'settings-pane-privacy'])
      await expect(canvas.queryByTestId(gone)).toBeNull()
    await expect(canvas.queryByTestId('settings-pane-notifications')).toBeNull()
  }}
/>

<!--
  Show Banners, by keyboard: off, an app's notification is not shown (and is not kept: turning them back on shows only
  what arrives from then on); the choice is stored.
-->
<Story
  name="Banners off"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-general'))
    const toggle = canvas.getByTestId('settings-banners')
    const notify = (title: string): void =>
      notifications.deliver({ app: FAKE_CALCULATOR.id, title })

    toggle.focus()
    await userEvent.keyboard(' ')
    await expect(toggle).toHaveAttribute('aria-checked', 'false')
    await expect(settings.banners).toBe(false)
    await expect(stored()).toMatchObject({ banners: false })
    notify('While off')
    await expect(notifications.banners).toHaveLength(0)

    await userEvent.keyboard('{Enter}')
    await expect(toggle).toHaveAttribute('aria-checked', 'true')
    notify('While on')
    await expect(notifications.banners.map((b) => b.title)).toEqual(['While on'])
    notifications.clear()

    await userEvent.click(toggle)
    await expect(toggle).toHaveAttribute('aria-checked', 'false')
    await expect(toggle).toHaveAccessibleName('Show Banners')
  }}
/>

<!-- Appearance, as a browser that has chosen nothing shows it. -->
<Story
  name="Appearance"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    await expect(canvas.getByTestId('settings-theme-auto')).toHaveAttribute('aria-checked', 'true')
    await expect(canvas.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'true')
    await expect(canvas.getByTestId('settings-text-size')).toHaveValue('100')
    await expect(canvas.getByTestId('settings-text-size-value')).toHaveTextContent('100%')
    await expect(canvas.getByTestId('settings-reduce-transparency')).toHaveAttribute(
      'aria-checked',
      'false'
    )
    await expect(stored()).toBeNull()
  }}
/>

<!-- Theme, by keyboard: the arrows move the choice, and the window turns dark whatever the system (Storybook) shows. -->
<Story
  name="Dark theme chosen"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    canvas.getByTestId('settings-theme-auto').focus()
    await userEvent.keyboard('{ArrowLeft}')
    const dark = canvas.getByTestId('settings-theme-dark')
    await expect(dark).toHaveAttribute('aria-checked', 'true')
    await expect(dark).toHaveFocus()
    await expect(canvas.getByTestId('settings-theme-auto')).toHaveAttribute('aria-checked', 'false')
    await expect(settings.appearance).toBe('dark')
    const page = canvas.getByTestId('settings')
    const schemeOf = (): string => getComputedStyle(page).colorScheme
    await waitFor(() => expect(schemeOf()).toBe('dark'))
    await expect(stored()).toMatchObject({ theme: 'dark' })

    // and Light, with the pointer, then back: the last choice is the one that holds
    await userEvent.click(canvas.getByTestId('settings-theme-light'))
    await waitFor(() => expect(schemeOf()).toBe('light'))
    await userEvent.click(dark)
    await waitFor(() => expect(schemeOf()).toBe('dark'))
  }}
/>

<!-- Accent colour, by keyboard: one swatch is pressed at a time, and the tint of the page follows. -->
<Story
  name="Green accent chosen"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    const green = canvas.getByTestId('settings-accent-green')
    const slider = canvas.getByTestId('settings-text-size')
    const tintOf = (): string => getComputedStyle(slider).getPropertyValue('--tint')
    const before = tintOf()
    green.focus()
    await userEvent.keyboard('{Enter}')
    await expect(green).toHaveAttribute('aria-pressed', 'true')
    await expect(canvas.getByTestId('settings-accent-blue')).toHaveAttribute(
      'aria-pressed',
      'false'
    )
    await expect(settings.accent).toBe('green')
    await waitFor(() => expect(tintOf()).not.toBe(before))
    await expect(document.documentElement.style.getPropertyValue('--tint')).toBe(
      'light-dark(#1f7a36, #30d158)'
    )
    await expect(stored()).toMatchObject({ accent: 'green' })
    // every swatch is big enough to hit
    await expect(green.getBoundingClientRect().width).toBeGreaterThanOrEqual(28)
  }}
/>

<!--
  Text size: the slider is a native range input (arrows, Home and End are the browser's; user-event does not emulate
  them, so the play function sends the input it makes). The text of the page itself grows.
-->
<Story
  name="Largest text"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    const slider = canvas.getByTestId('settings-text-size')
    const row = canvas.getByTestId('settings-text-size-value')
    await expect(fontSizeOf(row)).toBe(17)
    await fireEvent.input(slider, { target: { value: '125' } })
    await expect(row).toHaveTextContent('125%')
    await expect(settings.textScale).toBe(125)
    await waitFor(() => expect(fontSizeOf(row)).toBe(21.25))
    await expect(stored()).toMatchObject({ textScale: 125 })
    await expect(slider).toHaveAccessibleName('Text size, percent')
    await expect(slider).toHaveAttribute('min', '85')
    await expect(slider).toHaveAttribute('max', '125')
    await expect(slider).toHaveAttribute('step', '5')
  }}
/>

<Story
  name="Smallest text"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    const row = canvas.getByTestId('settings-text-size-value')
    await fireEvent.input(canvas.getByTestId('settings-text-size'), { target: { value: '85' } })
    await expect(row).toHaveTextContent('85%')
    await waitFor(() => expect(fontSizeOf(row)).toBeCloseTo(14.45, 2))
  }}
/>

<!-- Reduce Transparency, by keyboard: the bar over the pane stops blurring what is behind it. -->
<Story
  name="Transparency reduced"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    const toggle = canvas.getByTestId('settings-reduce-transparency')
    const bar = canvas.getByTestId('settings-nav').firstElementChild as HTMLElement
    await expect(getComputedStyle(bar).backdropFilter).toContain('blur')
    toggle.focus()
    await userEvent.keyboard(' ')
    await expect(toggle).toHaveAttribute('aria-checked', 'true')
    await expect(settings.reduceTransparency).toBe(true)
    await waitFor(() => expect(getComputedStyle(bar).backdropFilter).toBe('none'))
    await expect(document.documentElement).toHaveClass('reduce-transparency')
    await expect(stored()).toMatchObject({ reduceTransparency: true })
  }}
/>

<!-- What was chosen is what the next page load starts with (the journeys reload the real shell to show the same). -->
<Story
  name="Choices are kept for the next load"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-appearance'))
    await userEvent.click(canvas.getByTestId('settings-theme-light'))
    await userEvent.click(canvas.getByTestId('settings-accent-purple'))
    await fireEvent.input(canvas.getByTestId('settings-text-size'), { target: { value: '110' } })
    await userEvent.click(canvas.getByTestId('settings-reduce-transparency'))
    const next = new Settings()
    await expect({
      theme: next.theme,
      accent: next.accent,
      textScale: next.textScale,
      reduceTransparency: next.reduceTransparency,
      banners: next.banners
    }).toEqual({
      theme: 'light',
      accent: 'purple',
      textScale: 110,
      reduceTransparency: true,
      banners: true
    })
  }}
/>

<!--
  A window too narrow for two columns: the list of panes, then the pane with Back. By keyboard, focus goes to Back
  when a pane opens and to the pane's row on the way back.
-->
<Story
  name="Narrow window, one column at a time"
  tags={['!app']}
  args={{ width: 380, height: 520 }}
  play={async ({ canvas, userEvent }) => {
    await waitFor(() => expect(canvas.queryByTestId('settings-nav')).toBeNull())
    for (const id of ['general', 'appearance', 'devices'])
      await expect(canvas.getByTestId(`settings-pane-${id}`)).toBeVisible()

    canvas.getByTestId('settings-pane-appearance').focus()
    await userEvent.keyboard('{Enter}')
    await expect(await canvas.findByTestId('settings-theme')).toBeVisible()
    await expect(canvas.queryByTestId('settings-pane-appearance')).toBeNull()
    const back = canvas.getByTestId('settings-nav-back')
    await waitFor(() => expect(back).toHaveFocus())
    await expect(back).toHaveAccessibleName('Settings')

    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(canvas.getByTestId('settings-pane-appearance')).toHaveFocus())
    await expect(canvas.queryByTestId('settings-theme')).toBeNull()

    // and into another pane: this is the state the picture shows
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await expect(await canvas.findByTestId('settings-device-d2')).toBeVisible()
  }}
/>

<!-- The same window showing its first screen: the list. -->
<Story
  name="Narrow window, the list"
  tags={['!app']}
  args={{ width: 380, height: 520 }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.queryByTestId('settings-nav')).toBeNull())
    await expect(canvas.getByTestId('settings-pane-general')).toBeVisible()
  }}
/>

<Story
  name="Devices"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    const mine = await canvas.findByTestId('settings-device-d1')
    await expect(mine).toHaveTextContent('Safari on iPad')
    await expect(mine).toHaveTextContent('Using now')
    await expect(mine).toHaveTextContent('This device')
    await expect(canvas.queryByTestId('settings-device-d1-revoke')).toBeNull()
    await expect(canvas.getByTestId('settings-device-d2')).toHaveTextContent(
      'Last seen 3 hours ago'
    )
    await expect(canvas.getByTestId('settings-device-d2-revoke')).toHaveAccessibleName(
      'Revoke Chrome on Mac'
    )
    await expect(canvas.getByTestId('settings-device-d3-revoke')).toBeVisible()
    await expect(canvas.queryByTestId('settings-devices-error')).toBeNull()
    await expect(canvas.queryByTestId('settings-devices-empty')).toBeNull()
  }}
/>

<!--
  The confirmation: it names the device, starts on Cancel, keeps Tab inside, and Escape or Cancel closes it with
  nothing revoked and focus back on the Revoke that opened it.
-->
<Story
  name="Revoke device alert"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    const revoke = await canvas.findByTestId('settings-device-d2-revoke')
    const alert = (): HTMLElement | null => canvas.queryByTestId('settings-device-revoke-alert')
    const cancel = (): HTMLElement =>
      canvas.getByTestId('settings-device-revoke-alert-action-cancel')

    revoke.focus()
    await userEvent.keyboard('{Enter}')
    await expect(alert()).toHaveAccessibleName('Revoke Chrome on Mac?')
    await expect(alert()).toHaveAccessibleDescription(
      'It will be signed out of vos and must pair again to come back.'
    )
    await waitFor(() => expect(cancel()).toHaveFocus())
    await userEvent.tab()
    await expect(canvas.getByTestId('settings-device-revoke-alert-action-revoke')).toHaveFocus()
    await userEvent.tab()
    await expect(cancel()).toHaveFocus()

    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(alert()).toBeNull())
    await expect(revoke).toHaveFocus()
    await expect(canvas.getByTestId('settings-device-d2')).toBeVisible()

    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(cancel()).toHaveFocus())
    await userEvent.click(cancel())
    await waitFor(() => expect(alert()).toBeNull())
    await expect(revoke).toHaveFocus()
    await expect(canvas.getByTestId('settings-device-d2')).toBeVisible()

    // open, as the picture shows it
    await userEvent.click(revoke)
    await expect(alert()).toBeVisible()
  }}
/>

<!-- Confirmed: the device is gone from the list, and focus is on the Revoke of the device that took its place. -->
<Story
  name="Device revoked"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await userEvent.click(await canvas.findByTestId('settings-device-d2-revoke'))
    await userEvent.click(canvas.getByTestId('settings-device-revoke-alert-action-revoke'))
    await waitFor(() => expect(canvas.queryByTestId('settings-device-d2')).toBeNull())
    await expect(canvas.queryByTestId('settings-device-revoke-alert')).toBeNull()
    await expect(canvas.getByTestId('settings-device-d1')).toBeVisible()
    await waitFor(() => expect(canvas.getByTestId('settings-device-d3-revoke')).toHaveFocus())
    await expect(canvas.queryByTestId('settings-devices-error')).toBeNull()
  }}
/>

<!-- The last other device revoked, by keyboard: only this device is left, and focus is on the list, not lost. -->
<Story
  name="Only this device left"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    for (const id of ['d3', 'd2']) {
      ;(await canvas.findByTestId(`settings-device-${id}-revoke`)).focus()
      await userEvent.keyboard('{Enter}')
      await waitFor(() =>
        expect(canvas.getByTestId('settings-device-revoke-alert-action-cancel')).toHaveFocus()
      )
      await userEvent.tab()
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(canvas.queryByTestId(`settings-device-${id}`)).toBeNull())
      if (id === 'd3')
        await waitFor(() => expect(canvas.getByTestId('settings-device-d2-revoke')).toHaveFocus())
    }
    await waitFor(() => expect(canvas.getByTestId('settings-devices')).toHaveFocus())
    await expect(canvas.getByTestId('settings-devices')).toHaveAccessibleName('Paired devices')
    await expect(canvas.getByTestId('settings-device-d1')).toHaveTextContent('This device')
  }}
/>

<!-- The host refuses the revoke: the device stays, the page says which device and why, and focus stays on its Revoke. -->
<Story
  name="Device could not be revoked"
  beforeEach={withHost({ refuses: { 'host.devices.revoke': 1 } })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    const revoke = await canvas.findByTestId('settings-device-d2-revoke')
    await userEvent.click(revoke)
    await userEvent.click(canvas.getByTestId('settings-device-revoke-alert-action-revoke'))
    const error = await canvas.findByTestId('settings-devices-error')
    await expect(error).toHaveTextContent(
      'Chrome on Mac could not be revoked: Not connected to vos'
    )
    await expect(error).toHaveAttribute('role', 'alert')
    await expect(canvas.getByTestId('settings-device-d2')).toBeVisible()
    await expect(canvas.getByTestId('settings-device-d2-revoke')).toHaveFocus()
  }}
/>

<!-- The list cannot be read: the page says so and shows no list (an empty one would say nobody is paired). -->
<Story
  name="Devices could not be read"
  beforeEach={withHost({ refuses: { 'host.devices.list': Infinity } })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    const error = await canvas.findByTestId('settings-devices-error')
    await expect(error).toHaveTextContent(
      'The paired devices could not be read: Not connected to vos'
    )
    await expect(canvas.queryByTestId('settings-devices')).toBeNull()
    await expect(canvas.queryByTestId('settings-devices-empty')).toBeNull()
    await userEvent.click(canvas.getByTestId('settings-devices-retry'))
    await expect(await canvas.findByTestId('settings-devices-error')).toBeVisible()
  }}
/>

<!-- Try Again, once the host answers: the list, and the problem is gone. -->
<Story
  name="Devices read on a second try"
  tags={['play-only']}
  beforeEach={withHost({ refuses: { 'host.devices.list': 1 } })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    const retry = await canvas.findByTestId('settings-devices-retry')
    retry.focus()
    await userEvent.keyboard('{Enter}')
    await expect(await canvas.findByTestId('settings-device-d1')).toBeVisible()
    await expect(canvas.queryByTestId('settings-devices-error')).toBeNull()
  }}
/>

<!-- A host that lists no device at all (it always lists the one asking, so this is only what a fault would show). -->
<Story
  name="No paired devices"
  beforeEach={withHost({ devices: 0 })}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('settings-pane-devices'))
    await expect(await canvas.findByTestId('settings-devices-empty')).toHaveTextContent(
      'No paired devices'
    )
    await expect(canvas.queryByTestId('settings-devices-error')).toBeNull()
  }}
/>
