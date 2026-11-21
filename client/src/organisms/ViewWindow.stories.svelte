<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import ViewWindow from './ViewWindow.svelte'
  import { host, type FakeViews } from '../lib/host.svelte'
  import { createFakeHost, FAKE_CALCULATOR } from '../shell/fakeHost'
  import { TreeApp } from '../view/host/treeApp'
  import { openAppStory, until, type StoryHandle } from '../view/host/storyKit'
  import type { Node } from '../view/protocol'

  const size = FAKE_CALCULATOR.window!

  const ready: Node = {
    type: 'Screen',
    key: 'main',
    props: { title: 'Calculator' },
    children: [
      {
        type: 'Stack',
        key: '_0',
        props: { axis: 'v', padding: 4 },
        children: [{ type: 'Text', key: 'hello', props: { text: 'Ready' } }]
      }
    ]
  }

  /** How the host answers `view.open` in a story, and how many times the window asked. */
  let attempts = 0
  let sessions: StoryHandle<TreeApp>[] = []

  const openReady = async (): ReturnType<FakeViews['openView']> => {
    const session = await openAppStory(new TreeApp({ tree: ready }), { w: size.w, h: size.h })
    sessions.push(session)
    return session.view
  }

  type Behaviour = 'never' | 'fail' | 'fail-once' | 'fail-slow' | 'ok'
  const views = (behaviour: Behaviour): FakeViews => ({
    openView: () => {
      attempts++
      if (behaviour === 'never') return new Promise(() => undefined)
      // a retry that takes a moment to fail again: the alert gives way to the spinner and comes back
      if (behaviour === 'fail-slow' && attempts > 1)
        return new Promise((_, reject) =>
          setTimeout(() => reject(new Error('The app is not running on this host.')), 150)
        )
      if (behaviour === 'fail' || (behaviour === 'fail-once' && attempts === 1))
        return Promise.reject(new Error('The app is not running on this host.'))
      return openReady()
    },
    closeView: () => Promise.resolve()
  })

  const withViews = (behaviour: Behaviour) => () => {
    attempts = 0
    host.useFake(createFakeHost(), [FAKE_CALCULATOR], views(behaviour))
    return () => {
      for (const s of sessions.splice(0)) s.dispose()
      host.useFake(createFakeHost(), [FAKE_CALCULATOR])
    }
  }

  /** An installed app's window: opens a view on the host, shows it opening, and says so (with a retry) when it cannot. */
  const { Story } = defineMeta({
    title: 'Shell/App window',
    component: ViewWindow,
    tags: ['app'],
    args: { width: size.w, height: size.h },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={FAKE_CALCULATOR.name} {...args} minWidth={size.minW} minHeight={size.minH}>
    <ViewWindow appId="calculator" windowId="w1" kind="main" onclosewindow={() => undefined} />
  </AppWindow>
{/snippet}

<!-- The host has not answered yet: a spinner with the app's name for a screen reader. -->
<Story
  name="Opening"
  beforeEach={withViews('never')}
  play={async ({ canvas }) => {
    const opening = await canvas.findByTestId('calculator-opening')
    await expect(opening).toBeVisible()
    await expect(canvas.getByRole('status', { name: 'Opening Calculator' })).toBeVisible()
    await expect(canvas.queryByTestId('calculator-failed')).toBeNull()
  }}
/>

<!-- The host refused to open it: an alert says why, and "Try again" asks the host again. -->
<Story
  name="Failed"
  beforeEach={withViews('fail')}
  play={async ({ canvas }) => {
    const failed = await canvas.findByTestId('calculator-failed')
    await expect(failed).toHaveAttribute('role', 'alert')
    await expect(failed).toHaveTextContent('Couldn’t open Calculator')
    await expect(failed).toHaveTextContent('The app is not running on this host.')
    await expect(canvas.getByTestId('calculator-failed-retry')).toBeEnabled()
    // a target a finger can hit (44 pt)
    const rect = canvas.getByTestId('calculator-failed-retry').getBoundingClientRect()
    await expect(rect.height).toBeGreaterThanOrEqual(44)
  }}
/>

<!-- Try again: the failure goes, the window opens its view, and the host was asked a second time. -->
<Story
  name="Failed, then retry"
  beforeEach={withViews('fail-once')}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('calculator-failed-retry'))
    await expect(await canvas.findByTestId('calculator-view')).toBeVisible()
    await expect(await canvas.findByText('Ready')).toBeVisible()
    await expect(canvas.queryByTestId('calculator-failed')).toBeNull()
    await until(() => attempts === 2, 'the second open')
  }}
/>

<!-- Retry that fails again stays on the alert (and can be tried once more). -->
<Story
  name="Failed again"
  beforeEach={withViews('fail')}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('calculator-failed-retry'))
    await waitFor(() => expect(attempts).toBe(2))
    await expect(await canvas.findByTestId('calculator-failed')).toBeVisible()
    await expect(canvas.getByTestId('calculator-failed-retry')).toBeEnabled()
  }}
/>

<!-- Focus stays with the person: a retry that fails again puts it back on "Try again" (the old button went with the alert). -->
<Story
  name="Failed again keeps focus"
  beforeEach={withViews('fail-slow')}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('calculator-failed-retry'))
    await expect(await canvas.findByTestId('calculator-opening')).toBeVisible()
    await expect(await canvas.findByTestId('calculator-failed')).toBeVisible()
    await waitFor(() => expect(canvas.getByTestId('calculator-failed-retry')).toHaveFocus())
    await userEvent.keyboard('{Enter}') // and it works from the keyboard, again
    await until(() => attempts === 3, 'the third open')
    await waitFor(() => expect(canvas.getByTestId('calculator-failed-retry')).toHaveFocus())
  }}
/>
