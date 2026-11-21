<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor, within } from 'storybook/test'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import ViewHost from '../organisms/ViewHost.svelte'
  import type { FakeRun } from '../host/fakeAgui'
  import {
    openChatStory,
    storyThreadPath,
    type ChatScenario,
    type ChatStoryHandle
  } from '../host/chatStory'
  import { until } from '../host/storyKit'

  /** A control's hit area, in CSS pixels: every touch target is at least 44 x 44. */
  const expectHitArea = async (el: HTMLElement): Promise<void> => {
    const box = el.getBoundingClientRect()
    await expect(box.width).toBeGreaterThanOrEqual(44)
    await expect(box.height).toBeGreaterThanOrEqual(44)
  }

  /** The session of the story on screen, for its play function (the host side of the scenario). */
  let latest: Promise<ChatStoryHandle> | null = null
  const open = (scenario: ChatScenario): Promise<ChatStoryHandle> => {
    void latest?.then((previous) => previous.dispose()) // a re-render replaces the session; do not leak the old one
    return (latest = openChatStory(scenario))
  }
  const story = async (): Promise<ChatStoryHandle> => {
    await until(() => latest !== null, 'the story session')
    return latest!
  }

  /** What a run of each kind says. Stories pass them as scripts; the play function waits for each stage. */
  const reply =
    (text: string) =>
    async (run: FakeRun): Promise<void> => {
      run.start()
      await run.text(text, { chunk: 4, delay: 70 })
      run.finish()
    }

  const toolTurn = async (run: FakeRun): Promise<void> => {
    run.start()
    await run.thought('The user wants a total: I will use the calculator.')
    await run.tool('mcp__agentos__calculator__calculate', { kind: 'execute', delay: 30 })
    await run.tool('Read', { kind: 'read', delay: 30 })
    await run.tool('mcp__agentos__notes__append', { kind: 'edit', status: 'failed' })
    run.patch([{ op: 'replace', path: '/session/usage', value: { used: 61000, size: 200000 } }])
    await run.text('18% of 2,450 is **441**. I could not save it to Notes.', {
      chunk: 12,
      delay: 10
    })
    run.finish()
  }

  const permissionTurn = (run: FakeRun): void => {
    run.start()
    run.interrupt({ id: 'int-1', title: 'mcp__agentos__calculator__show_on_calculator' })
  }
  const afterPermission = async (run: FakeRun): Promise<void> => {
    run.start()
    await run.tool('mcp__agentos__calculator__show_on_calculator', { kind: 'execute' })
    await run.text('Done: it is on the calculator.')
    run.finish()
  }

  const history = [
    { id: 'u1', role: 'user', content: 'What is 18% of 2,450? Show it on the calculator.' },
    {
      id: 'c1',
      role: 'assistant',
      toolCalls: [
        {
          id: 'tc1',
          type: 'function',
          function: { name: 'mcp__agentos__calculator__show_on_calculator', arguments: '{}' }
        }
      ],
      agentos: {
        title: 'mcp__agentos__calculator__show_on_calculator',
        kind: 'execute',
        status: 'completed'
      }
    },
    { id: 'tr1', role: 'tool', toolCallId: 'tc1', content: 'ok' },
    {
      id: 'a1',
      role: 'assistant',
      content:
        '18% of 2,450 is **441**. It is on the Calculator now:\n- 2450 x 0.18\n- result `441`'
    },
    { id: 'u2', role: 'user', content: 'Thanks!' },
    { id: 'a2', role: 'assistant', content: 'Any time.' }
  ]

  /**
   * The chat transcript: a `ChatTranscript` node in the managed renderer, with the messages arriving over a scripted
   * `/agui` host (HTTP + SSE as `fetch`) while the window itself runs on the in-memory `/rpc` host.
   */
  const { Story } = defineMeta({
    title: 'Renderer/Chat transcript',
    component: ViewHost,
    tags: ['app'],
    args: { width: 520, height: 720, scenario: {} as ChatScenario },
    argTypes: { scenario: { control: false } },
    render: template
  })

  interface Canvas {
    findByTestId: (id: string) => Promise<HTMLElement>
  }
  interface User {
    type: (element: Element, text: string) => Promise<void>
    click: (element: Element) => Promise<void>
  }

  const chat = (canvas: Canvas, part: string): Promise<HTMLElement> =>
    canvas.findByTestId(`agents-chat-${part}`)

  async function say(canvas: Canvas, userEvent: User, text: string): Promise<void> {
    const input = await canvas.findByTestId('agents-chat-composer-input')
    await waitFor(() => expect(input).toBeEnabled())
    await userEvent.type(input, text)
    await userEvent.click(await canvas.findByTestId('agents-chat-composer-send'))
  }
</script>

{#snippet template(args: { width: number; height: number; scenario: ChatScenario })}
  <AppWindow title="Agents" width={args.width} height={args.height} minWidth={320} minHeight={240}>
    {#await open(args.scenario)}
      <p class="loading">Connecting…</p>
    {:then s}
      <div class="fill" {@attach () => () => s.dispose()}>
        <ViewHost view={s.view} app="agents" registry={s.registry} />
      </div>
    {/await}
  </AppWindow>
{/snippet}

<!-- A new chat: the thread is opened on the host, the session bar offers mode, model, effort, commands and usage. -->
<Story
  name="New chat"
  play={async ({ canvas, userEvent }) => {
    await chat(canvas, 'thread')
    await waitFor(() => expect(canvas.getByText('Ask Claude')).toBeVisible())
    const model = await chat(canvas, 'session-config-model')
    await expect(model).toHaveValue('large')
    for (const part of ['composer-attach', 'composer-send'])
      await expectHitArea(await chat(canvas, part))
    await expect(await chat(canvas, 'session-mode')).toHaveValue('default')
    await expect(await chat(canvas, 'session-usage')).toBeVisible()
    await userEvent.selectOptions(await chat(canvas, 'session-config-effort'), 'high')
    const { agui } = await story()
    await waitFor(() =>
      expect(agui.callsTo('POST', `${storyThreadPath()}/settings`).at(-1)?.body).toEqual({
        config: { id: 'effort', value: 'high' }
      })
    )
    await waitFor(() =>
      expect(canvas.getByTestId('agents-chat-session-config-effort-value')).toHaveTextContent(
        'High'
      )
    )
  }}
/>

<!-- A reply streams in: the text grows chunk by chunk, the turn shows one live activity line, then it settles. -->
<Story
  name="Streaming reply"
  args={{
    scenario: {
      onRun: reply(
        'Hello! I can help with that. Here is a short answer that arrives piece by piece.'
      )
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Hi there')
    await waitFor(() => expect(canvas.getByText('Hi there')).toBeVisible())
    await expect(await chat(canvas, 'composer-stop')).toBeVisible()
    await expectHitArea(await chat(canvas, 'composer-stop'))
    // the log is busy while the reply streams, so a screen reader reads the finished message once
    await expect(canvas.getByTestId('agents-chat-thread')).toHaveAttribute('aria-busy', 'true')
    await waitFor(() =>
      expect(within(canvas.getByTestId('agents-chat-thread')).getByText('Working')).toBeVisible()
    )
    await waitFor(() => expect(canvas.getByText(/piece by piece/)).toBeVisible(), { timeout: 4000 })
    await waitFor(() => expect(canvas.queryByTestId('agents-chat-composer-stop')).toBeNull())
    await expect(canvas.getByTestId('agents-chat-thread')).toHaveAttribute('aria-busy', 'false')
    const { app } = await story()
    await expect(app.events.map((e) => e.event)).toEqual(['send'])
    await expect(app.events[0].payload).toEqual({ text: 'Hi there' })
  }}
/>

<!-- Stop: the run is cancelled on the host and the composer comes back. -->
<Story
  name="Stopped"
  args={{
    scenario: {
      onRun: async (run) => {
        run.start()
        await run.text(
          'This will take a very long time to say, so I will be stopped half way through it all',
          {
            chunk: 3,
            delay: 60
          }
        )
        run.finish()
      }
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Tell me a long story')
    await waitFor(() => expect(canvas.getByText(/This will/)).toBeVisible())
    await userEvent.click(await chat(canvas, 'composer-stop'))
    const { agui, app } = await story()
    await waitFor(() => expect(agui.callsTo('POST', `${storyThreadPath()}/cancel`)).toHaveLength(1))
    await waitFor(() => expect(canvas.queryByTestId('agents-chat-composer-stop')).toBeNull())
    await expect(app.events.map((e) => e.event)).toEqual(['send', 'stop'])
  }}
/>

<!-- Tool calls: one activity line with counts (reasoning, tool calls, failures), never the list of tools. -->
<Story
  name="Tool call"
  args={{ scenario: { onRun: toolTurn } }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'What is 18% of 2,450?')
    await waitFor(() => expect(canvas.getByText(/441/)).toBeVisible(), { timeout: 4000 })
    await waitFor(() =>
      expect(
        within(canvas.getByTestId('agents-chat-thread')).getByRole('status')
      ).toHaveTextContent('1 reasoning step · 3 tool calls · 1 failed')
    )
    await expect(canvas.queryByText(/calculator__|Calculator · /)).toBeNull()
    await waitFor(() => expect(canvas.getByTestId('agents-chat-session-usage')).toBeVisible())
  }}
/>

<!-- A permission prompt ends the run with an interrupt; "Allow once" resumes the turn and the rest streams in. -->
<Story
  name="Permission prompt"
  args={{ scenario: { onRun: permissionTurn, onResume: afterPermission } }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Put 441 on the calculator')
    const card = await canvas.findByRole('group', { name: 'Permission request' })
    await expect(card).toHaveTextContent('Allow Calculator · show on calculator?')
    await expect(
      canvas.getByTestId('agents-chat-thread-permission-permission-int-1-allow')
    ).toBeVisible()
    await expect(
      canvas.getByTestId('agents-chat-thread-permission-permission-int-1-allow-always')
    ).toBeVisible()
    await expect(
      canvas.getByTestId('agents-chat-thread-permission-permission-int-1-reject')
    ).toBeVisible()
    await userEvent.click(
      canvas.getByTestId('agents-chat-thread-permission-permission-int-1-allow')
    )
    await waitFor(() => expect(canvas.getByText('Done: it is on the calculator.')).toBeVisible())
    await expect(card).toHaveTextContent('Allow once')
    const { agui } = await story()
    await expect(agui.callsTo('POST', '/agui')[1].body).toMatchObject({
      resume: [{ interruptId: 'int-1', status: 'resolved', payload: { optionId: 'allow' } }]
    })
  }}
/>

<!-- Deny: the same prompt answered with the reject option; the agent carries on without the tool. -->
<Story
  name="Permission denied"
  args={{
    scenario: {
      onRun: permissionTurn,
      onResume: async (run) => {
        run.start()
        await run.text('Understood, I did not run it.')
        run.finish()
      }
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Put 441 on the calculator')
    await canvas.findByRole('group', { name: 'Permission request' })
    await userEvent.click(
      canvas.getByTestId('agents-chat-thread-permission-permission-int-1-reject')
    )
    await waitFor(() => expect(canvas.getByText('Understood, I did not run it.')).toBeVisible())
    await expect(canvas.getByRole('group', { name: 'Permission request' })).toHaveTextContent(
      'Deny'
    )
  }}
/>

<!-- An agent that repeats an option id (or asks twice with one interrupt id) must not crash the transcript: repeats are dropped at the edge. -->
<Story
  name="Permission with repeated ids"
  args={{
    scenario: {
      onRun: (run) => {
        run.start()
        run.interrupt({
          id: 'int-1',
          title: 'Bash',
          options: [
            { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
            { optionId: 'allow', name: 'Allow (again)', kind: 'allow_always' },
            { optionId: 'reject', name: 'Deny', kind: 'reject_once' }
          ]
        })
      }
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Run it')
    const card = await canvas.findByRole('group', { name: 'Permission request' })
    await expect(within(card).getAllByRole('button')).toHaveLength(2)
    await expect(within(card).getByText('Allow once')).toBeVisible()
    await expect(within(card).queryByText('Allow (again)')).toBeNull()
  }}
/>

<!-- The agent fails mid-run: an error line in the transcript, and the chat is usable again. -->
<Story
  name="Error"
  args={{
    scenario: {
      onRun: async (run) => {
        run.start()
        await run.text('Let me look into that')
        run.error('The agent exited unexpectedly (code 1).', 'agent_exited')
      }
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await say(canvas, userEvent, 'Please help')
    const alert = await canvas.findByRole('alert')
    await expect(alert).toHaveTextContent('The agent exited unexpectedly (code 1).')
    await waitFor(() => expect(canvas.queryByTestId('agents-chat-composer-stop')).toBeNull())
    await expect(await chat(canvas, 'composer-input')).toBeEnabled()
  }}
/>

<!-- The thread could not be started: the empty state says why and offers to try again (which then works). -->
<Story
  name="Agent unavailable"
  args={{ scenario: { openError: 'Claude could not start: command not found.' } }}
  play={async ({ canvas, userEvent }) => {
    await expect(await canvas.findByRole('alert')).toHaveTextContent(
      'Claude could not start: command not found.'
    )
    await expect(canvas.getByText(/Could not start/)).toBeVisible()
    const { agui } = await story()
    agui.overrides.delete('POST /agui/threads')
    await userEvent.click(await chat(canvas, 'retry'))
    await waitFor(() => expect(canvas.getByText(/^Ask /)).toBeVisible())
  }}
/>

<!-- A chat with history: the earlier turns come from the host's snapshot, with one activity line per turn. -->
<Story
  name="Finished chat"
  args={{ scenario: { history } }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByText('Thanks!')).toBeVisible())
    await expect(canvas.getByText('Any time.')).toBeVisible()
    await expect(
      within(canvas.getByTestId('agents-chat-thread')).getByRole('status')
    ).toHaveTextContent('1 tool call')
    await expect(canvas.getByText('441', { selector: 'strong' })).toBeVisible()
  }}
/>

<!-- A permission still pending when the chat is opened (after a reload): the prompt is offered again. -->
<Story
  name="Pending permission after reload"
  args={{
    scenario: {
      history: history.slice(0, 1),
      interrupts: [
        {
          id: 'int-9',
          reason: 'tool_call',
          message: 'Bash',
          metadata: {
            'agentos:options': [
              { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
              { optionId: 'reject', name: 'Deny', kind: 'reject_once' }
            ]
          }
        }
      ]
    }
  }}
  play={async ({ canvas }) => {
    await expect(
      await canvas.findByRole('group', { name: 'Permission request' })
    ).toHaveTextContent('Allow Bash?')
    await waitFor(() => expect(canvas.getByTestId('agents-chat-composer-input')).toBeDisabled())
  }}
/>

<!-- Slash commands: typing / in the composer lists the agent's commands; Enter picks one. -->
<Story
  name="Slash commands"
  play={async ({ canvas, userEvent }) => {
    const input = await canvas.findByTestId('agents-chat-composer-input')
    await waitFor(() => expect(input).toBeEnabled())
    await userEvent.type(input, '/co')
    await expect(await canvas.findByRole('listbox', { name: 'Slash commands' })).toBeVisible()
    await expect(canvas.getByTestId('agents-chat-composer-command-compact')).toBeVisible()
    await expect(canvas.queryByTestId('agents-chat-composer-command-init')).toBeNull()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(input).toHaveValue('/compact '))
  }}
/>

<!-- The smallest window: the session bar wraps, nothing overflows sideways. -->
<Story
  name="Narrow window"
  args={{ width: 320, height: 560, scenario: { history } }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByText('Any time.')).toBeVisible())
    await expect(await chat(canvas, 'session-config-model-value')).toBeVisible()
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
