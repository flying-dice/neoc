<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import ViewHost from '../organisms/ViewHost.svelte'
  import { openStory, until, type StoryHandle, type StoryScenario } from '../host/storyKit'
  import type { Node } from '../protocol'

  /** The session of the story on screen, for its play function (the host side of the scenario). */
  let latest: Promise<StoryHandle> | null = null
  const open = (scenario: StoryScenario): Promise<StoryHandle> => {
    void latest?.then((previous) => previous.dispose()) // a re-render replaces the session; do not leak the old one
    return (latest = openStory(scenario))
  }
  /** The story on screen, once its session is open (a play function can start before the await block resolves). */
  const story = async (): Promise<StoryHandle> => {
    await until(() => latest !== null, 'the story session')
    return latest!
  }

  const unknownNodes: Node[] = [
    { type: 'Gauge', key: 'gauge', props: { value: 0.4, label: 'Storage' } },
    {
      type: 'Gauge',
      key: 'gauge-fallback',
      props: { value: 0.4, label: 'Storage' },
      slots: {
        fallback: [
          {
            type: 'Text',
            key: 'fallback-text',
            props: { text: 'Storage: 40% used', style: 'footnote' }
          }
        ]
      }
    }
  ]

  /**
   * The managed renderer: a server-driven view tree rendered with the kit, against a TypeScript fake host on an
   * in-memory wire (the real path against the Rust host comes in wave B2). Every story drives the host side from
   * its play function, the way an agent or a dropped network would.
   */
  const { Story } = defineMeta({
    title: 'Renderer/Notes window',
    component: ViewHost,
    tags: ['app'],
    args: { width: 480, height: 720, scenario: {} as StoryScenario },
    argTypes: { scenario: { control: false } },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number; scenario: StoryScenario })}
  <AppWindow title="Notes" width={args.width} height={args.height} minWidth={320} minHeight={240}>
    {#await open(args.scenario)}
      <p class="loading">Connecting…</p>
    {:then s}
      <div class="fill" {@attach () => () => s.dispose()}>
        <ViewHost view={s.view} app="notes" onretry={() => s.harness.session.retryNow()} />
      </div>
    {/await}
  </AppWindow>
{/snippet}

<!-- A full tree: layout, text, text fields, toggles and a list bound to a source, all from JSON. -->
<Story
  name="Tree"
  play={async ({ canvas }) => {
    await canvas.findByTestId('notes-count')
    await waitFor(() => expect(canvas.getByTestId('notes-count')).toHaveTextContent('2 notes'))
    await expect(canvas.getByTestId('notes-list-row-1-item')).toBeVisible()
    await expect(canvas.getByTestId('notes-list-row-2-item')).toBeVisible()
    await expect(canvas.getByTestId('notes-body')).toBeVisible()
  }}
/>

<!-- Keyed ops: an event changes structure; the empty state is inserted by key and removed again. -->
<Story
  name="Ops applied"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('notes-actions-delete-all'))
    await expect(await canvas.findByTestId('notes-empty-state')).toBeVisible()
    await waitFor(() =>
      expect(canvas.getByTestId('notes-status')).toHaveTextContent('Deleted all notes')
    )
    await waitFor(() => expect(canvas.getByTestId('notes-count')).toHaveTextContent('0 notes'))
    const { harness } = await story()
    harness.app.agentAddNote(harness.host, 'Back again')
    await waitFor(() => expect(canvas.queryByTestId('notes-empty-state')).toBeNull())
    await waitFor(() => expect(canvas.getByTestId('notes-count')).toHaveTextContent('1 note'))
  }}
/>

<!-- Two-way binding: type, see "Saving…", one debounced write; an agent appends while typing and the host merges. -->
<Story
  name="Two-way binding"
  play={async ({ canvas, userEvent }) => {
    const body = await canvas.findByTestId('notes-body')
    await userEvent.type(body, 'Buy oat milk')
    await expect(await canvas.findByText('Saving…')).toBeVisible()
    const { harness } = await story()
    harness.app.agentAppendDraft(harness.host, 'agent: ')
    await expect(body).toHaveValue('Buy oat milk') // shielded: the caret and text are not clobbered mid-typing
    await until(() => harness.host.stateValue('body') === 'agent: Buy oat milk', 'the host merge')
    await waitFor(() => expect(body).toHaveValue('agent: Buy oat milk'))
    await waitFor(() => expect(canvas.queryByText('Saving…')).toBeNull())
  }}
/>

<!-- 300 rows in windows of 20: past 200 the list is virtualized. A few rows are in the DOM; scrolling far asks the host for the window; an agent adding a note sends one row. -->
<Story
  name="Windowed list"
  args={{ scenario: { notes: { extra: 298, page: 20 } } }}
  play={async ({ canvas, canvasElement }) => {
    await canvas.findByTestId('notes-count')
    await waitFor(() => expect(canvas.getByTestId('notes-count')).toHaveTextContent('300 notes'))
    const rows = (): number => canvas.queryAllByTestId(/^notes-list-row-\d+-item$/).length
    await waitFor(() => expect(rows()).toBeGreaterThan(3))
    await expect(rows()).toBeLessThan(40) // not 300
    const scroller = canvasElement.querySelector<HTMLElement>('.scroll')!
    scroller.scrollTop = scroller.scrollHeight
    await canvas.findByText('Note 3', undefined, { timeout: 4000 }) // the oldest row, a window the host sent on demand
    await expect(rows()).toBeLessThan(40)
    const { harness } = await story()
    harness.app.agentAddNote(harness.host, 'Newest note')
    await waitFor(() => expect(canvas.getByTestId('notes-count')).toHaveTextContent('301 notes'))
    scroller.scrollTop = 0
    await waitFor(() => expect(canvas.getByText('Newest note')).toBeVisible(), { timeout: 4000 })
  }}
/>

<!-- Overlapping edits: the human wins, the host keeps the other side, and the person is told. -->
<Story
  name="Conflict"
  play={async ({ canvas, userEvent }) => {
    const { harness } = await story()
    harness.host.setState('body', 'buy milk')
    const body = await canvas.findByTestId('notes-body')
    await waitFor(() => expect(body).toHaveValue('buy milk'))
    await userEvent.clear(body)
    await userEvent.type(body, 'buy soy')
    harness.host.setState('body', 'buy tea') // an agent replaces the same words while the field is dirty
    await expect(
      await canvas.findByText('Your edit replaced a change made elsewhere.')
    ).toBeVisible()
    await waitFor(() => expect(body).toHaveValue('buy soy'))
    await expect(harness.host.overwritten[0]).toMatchObject({ theirs: 'buy tea', mine: 'buy soy' })
  }}
/>

<!-- A strict source refuses a stale write: the server value is adopted and the field shows why. -->
<Story
  name="Refused write"
  args={{ scenario: { notes: { approvedMerge: 'reject' } } }}
  play={async ({ canvas, userEvent }) => {
    const { harness } = await story()
    const approved = await canvas.findByTestId('notes-approved')
    harness.host.setState('approved', true)
    await waitFor(() => expect(approved).toHaveAttribute('aria-checked', 'true'))
    // The person turns it off while the host moves on behind their back, before the write arrives.
    harness.link.setLatency(150)
    await userEvent.click(approved)
    harness.host.setState('approved', true)
    await waitFor(() => expect(canvas.getByText('Changed elsewhere')).toBeVisible())
    await waitFor(() => expect(approved).toHaveAttribute('aria-checked', 'true'))
    harness.link.setLatency(0)
  }}
/>

<!-- The connection drops: the window dims with a banner, typing still works, edits wait to be sent. -->
<Story
  name="Offline"
  play={async ({ canvas, userEvent }) => {
    const { harness } = await story()
    const body = await canvas.findByTestId('notes-body')
    harness.link.setReachable(false)
    harness.link.drop()
    await expect(await canvas.findByTestId('notes-offline')).toBeVisible()
    await userEvent.type(body, 'Typed while offline')
    await waitFor(() => expect(body).toHaveValue('Typed while offline'))
    await waitFor(() =>
      expect(canvas.getByTestId('notes-offline')).toHaveTextContent('1 edit is saved here')
    )
    await expect(canvas.getByTestId('notes-actions-save')).toBeDisabled() // events are never queued: a tap is not idempotent
  }}
/>

<!-- The connection comes back: the same instance resumes, queued edits replay on their base and merge. -->
<Story
  name="Reconnected"
  play={async ({ canvas, userEvent }) => {
    const { harness } = await story()
    const body = await canvas.findByTestId('notes-body')
    harness.link.setReachable(false)
    harness.link.drop()
    await canvas.findByTestId('notes-offline')
    await userEvent.type(body, 'draft')
    harness.host.setState('body', '+agent ') // changed while the client was away
    harness.link.setReachable(true)
    harness.session.retryNow()
    await waitFor(() => expect(canvas.queryByTestId('notes-offline')).toBeNull(), { timeout: 4000 })
    await waitFor(() => expect(body).toHaveValue('+agent draft'))
    await expect(harness.host.stateValue('body')).toBe('+agent draft')
  }}
/>

<!-- A component this client does not know: the placeholder, or the node's own fallback slot. -->
<Story
  name="Unsupported component"
  args={{ scenario: { notes: { extraNodes: unknownNodes } } }}
  play={async ({ canvas }) => {
    await canvas.findByTestId('notes-gauge')
    await waitFor(() =>
      expect(canvas.getByTestId('notes-gauge')).toHaveTextContent('Unsupported: Gauge')
    )
    await expect(canvas.getByTestId('notes-gauge-fallback-fallback-text')).toHaveTextContent(
      'Storage: 40% used'
    )
  }}
/>

<!-- The host reports an error the window cannot handle: an alert banner says what, and the last good tree stays on screen. -->
<Story
  name="Host error banner"
  play={async ({ canvas }) => {
    await canvas.findByTestId('notes-count')
    const { harness } = await story()
    harness.host.allViews()[0]!.push('view.error', { message: 'The app ran out of memory.' })
    const banner = await canvas.findByTestId('notes-view-error')
    await expect(banner).toHaveAttribute('role', 'alert')
    await expect(banner).toHaveTextContent('The app ran out of memory.')
    await expect(canvas.getByTestId('notes-tree')).toBeVisible()
    await expect(canvas.getByTestId('notes-count')).toBeVisible()
  }}
/>

<!-- The host closes the window (the app stopped or was removed): the tree gives way to a message, announced as an alert. -->
<Story
  name="Window closed by the host"
  play={async ({ canvas }) => {
    await canvas.findByTestId('notes-count')
    const { harness } = await story()
    harness.host.allViews()[0]!.push('view.closed', { reason: 'The app stopped.' })
    const closed = await canvas.findByTestId('notes-view-closed')
    await expect(closed).toHaveAttribute('role', 'alert')
    await expect(closed).toHaveTextContent('This window is no longer available')
    await expect(closed).toHaveTextContent('The app stopped.')
    await expect(canvas.queryByTestId('notes-tree')).toBeNull()
  }}
/>

<style>
  .fill {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .loading {
    margin: 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
