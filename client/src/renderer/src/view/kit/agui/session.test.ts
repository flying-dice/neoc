import { describe, expect, test } from 'bun:test'
import { FakeAgui, type FakeRun } from '../../host/fakeAgui'
import { liveActivityId, toBlocks } from './blocks'
import { ChatSession, friendlyTool, itemsFromMessages, scopedThread } from './session'
import { parseThreadState, type ThreadState } from './schema'

const sessionState = (): ThreadState =>
  parseThreadState({
    session: {
      agent: { name: 'claude-agent-acp', version: '0.108.0' },
      modes: {
        current: 'default',
        available: [
          { id: 'default', name: 'Default' },
          { id: 'plan', name: 'Plan' }
        ]
      },
      config: [
        {
          id: 'model',
          name: 'Model',
          category: 'model',
          kind: 'select',
          current: 'large',
          options: [
            { value: 'large', name: 'Large' },
            { value: 'small', name: 'Small' }
          ]
        },
        { id: 'fast', name: 'Fast', kind: 'boolean', current: false }
      ],
      commands: [{ name: 'compact', description: 'Compact the context' }],
      usage: { used: 1000, size: 200000 }
    },
    plan: null
  })

function setup(o: { bytesPerChunk?: number } = {}): { fake: FakeAgui; chat: ChatSession } {
  const fake = new FakeAgui({ state: sessionState(), ...o })
  let n = 0
  const chat = new ChatSession({
    threadId: 't1',
    agentId: 'claude',
    fetch: fake.fetch,
    newId: () => `id${++n}`
  })
  return { fake, chat }
}

async function until(check: () => boolean, label = 'condition'): Promise<void> {
  const start = Date.now()
  while (!check()) {
    if (Date.now() - start > 2000) throw new Error(`timed out waiting for ${label}`)
    await new Promise((r) => setTimeout(r, 5))
  }
}

const kinds = (chat: ChatSession): string[] => chat.getState().items.map((i) => i.kind)

describe('thread ids are scoped to the app that names them', () => {
  test('an app\'s thread id gets the app as a prefix, so two apps naming the same id get different threads', () => {
    expect(scopedThread('agents', 't1')).toBe('agents:t1')
    expect(scopedThread('notes', 't1')).not.toBe(scopedThread('agents', 't1'))
  })

  test('an app cannot reach another app\'s thread by putting the other prefix in its id', () => {
    // the prefix is always the opener's: "notes" naming "agents:t1" gets notes:agents:t1, never agents:t1
    expect(scopedThread('notes', 'agents:t1')).toBe('notes:agents:t1')
  })

  test('with no app (a story) or no id the id is unchanged', () => {
    expect(scopedThread(undefined, 't1')).toBe('t1')
    expect(scopedThread('agents', undefined)).toBeUndefined()
  })
})

describe('opening a thread', () => {
  test('ensures the thread on the host and loads state', async () => {
    const { fake, chat } = setup()
    expect(chat.getState().status).toBe('connecting')
    await chat.open()
    const s = chat.getState()
    expect(s.ready).toBe(true)
    expect(s.status).toBe('idle')
    expect(s.state.session.modes?.current).toBe('default')
    expect(s.state.session.config).toHaveLength(2)
    expect(fake.callsTo('POST', '/agui/threads')[0].body).toEqual({
      threadId: 't1',
      agentId: 'claude'
    })
  })

  test('restores history, tool status and a pending permission from the snapshot', async () => {
    const { fake, chat } = setup()
    const t = fake.thread('t1')
    t.messages = [
      { id: 'u1', role: 'user', content: 'hi' },
      { id: 'r1', role: 'reasoning', content: 'hm' },
      {
        id: 'c1',
        role: 'assistant',
        toolCalls: [
          {
            id: 'tc1',
            type: 'function',
            function: { name: 'mcp__agentos__notes__search', arguments: '{}' }
          }
        ],
        agentos: { title: 'mcp__agentos__notes__search', kind: 'read', status: 'failed' }
      },
      { id: 'tr1', role: 'tool', toolCallId: 'tc1', content: 'x' },
      { id: 'a1', role: 'assistant', content: 'done' },
      { id: 's1', role: 'system', content: 'it broke', agentos: { error: true } }
    ]
    t.interrupts = [
      {
        id: 'i1',
        reason: 'tool_call',
        message: 'Run it',
        metadata: { 'agentos:options': [{ optionId: 'ok', name: 'Allow', kind: 'allow_once' }] }
      }
    ]
    await chat.open()
    const s = chat.getState()
    expect(kinds(chat)).toEqual(['user', 'thought', 'tool', 'agent', 'error', 'permission'])
    expect(s.items[2]).toMatchObject({
      title: 'Notes · search',
      toolKind: 'read',
      status: 'failed'
    })
    expect(s.status).toBe('waiting')
    expect(s.items[5]).toMatchObject({ interruptId: 'i1', options: [{ optionId: 'ok' }] })
  })

  test('a running thread is shown as running', async () => {
    const { fake, chat } = setup()
    fake.thread('t1').running = true
    await chat.open()
    expect(chat.getState().status).toBe('running')
  })

  test('a host error opens as an error state with its message, and open can be retried', async () => {
    const { fake, chat } = setup()
    let fail = true
    fake.overrides.set('POST /agui/threads', () =>
      fail ? Response.json({ error: 'the agent could not start' }, { status: 502 }) : undefined
    )
    await chat.open()
    expect(chat.getState()).toMatchObject({
      status: 'error',
      ready: false,
      error: 'the agent could not start'
    })
    fail = false
    await chat.open()
    expect(chat.getState()).toMatchObject({ status: 'idle', ready: true, error: null })
  })

  test('send opens the thread first when it is not open yet', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => (run.start(), run.finish())
    await chat.send('hello')
    expect(fake.calls[0].path).toBe('/agui/threads')
    expect(fake.calls[1].path).toBe('/agui')
  })
})

describe('a run', () => {
  test('streams a text reply into one agent item, then goes idle', async () => {
    const { fake, chat } = setup()
    fake.onRun = async (run) => {
      run.start()
      await run.text('Hello there', { chunk: 3 })
      run.finish()
    }
    await chat.open()
    await chat.send('hi')
    const s = chat.getState()
    expect(kinds(chat)).toEqual(['user', 'agent'])
    expect(s.items[0]).toMatchObject({ text: 'hi' })
    expect(s.items[1]).toMatchObject({ text: 'Hello there', streaming: false })
    expect(s.status).toBe('idle')
    const body = fake.callsTo('POST', '/agui')[0].body as Record<string, unknown>
    expect(body).toMatchObject({
      threadId: 't1',
      messages: [{ role: 'user', content: 'hi' }],
      forwardedProps: { agentId: 'claude' }
    })
  })

  test('shows the text as it arrives (running, then idle)', async () => {
    const { fake, chat } = setup()
    let release!: () => void
    const gate = new Promise<void>((r) => (release = r))
    fake.onRun = async (run) => {
      run.start()
      run.send({ type: 'TEXT_MESSAGE_START', messageId: 'm', role: 'assistant' })
      run.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: 'Par' })
      await gate
      run.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: 'tial' })
      run.send({ type: 'TEXT_MESSAGE_END', messageId: 'm' })
      run.finish()
    }
    await chat.open()
    const sent = chat.send('go')
    await until(() => chat.getState().items.some((i) => i.kind === 'agent' && i.text === 'Par'))
    expect(chat.getState().status).toBe('running')
    expect(chat.getState().items.at(-1)).toMatchObject({ streaming: true })
    release()
    await sent
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'Partial', streaming: false })
  })

  test('frames split at any byte boundary and keep-alives do not matter', async () => {
    const { fake, chat } = setup({ bytesPerChunk: 5 })
    fake.onRun = async (run) => {
      run.ping()
      run.start()
      await run.text('héllo wörld')
      run.ping()
      run.finish()
    }
    await chat.open()
    await chat.send('x')
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'héllo wörld' })
  })

  test('reasoning becomes a thought item', async () => {
    const { fake, chat } = setup()
    fake.onRun = async (run) => {
      run.start()
      await run.thought('thinking it over')
      await run.text('ok')
      run.finish()
    }
    await chat.open()
    await chat.send('q')
    expect(kinds(chat)).toEqual(['user', 'thought', 'agent'])
    expect(chat.getState().items[1]).toMatchObject({ text: 'thinking it over' })
  })

  test('a tool call goes pending, in progress, then completed through agentos.tool', async () => {
    const { fake, chat } = setup()
    const seen: string[] = []
    chat.subscribe(() => {
      const t = chat.getState().items.find((i) => i.kind === 'tool')
      if (t?.kind === 'tool' && seen.at(-1) !== t.status) seen.push(t.status)
    })
    fake.onRun = async (run) => {
      run.start()
      await run.tool('mcp__agentos__calculator__show_on_calculator', { kind: 'execute' })
      run.finish()
    }
    await chat.open()
    await chat.send('calc')
    expect(seen).toEqual(['pending', 'in_progress', 'completed'])
    expect(chat.getState().items[1]).toMatchObject({
      kind: 'tool',
      title: 'Calculator · show on calculator',
      toolKind: 'execute',
      status: 'completed'
    })
  })

  test('a custom tool event for an unknown call creates the item', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => {
      run.start()
      run.send({
        type: 'CUSTOM',
        name: 'agentos.tool',
        value: { toolCallId: 'z', title: 'Bash', status: 'failed' }
      })
      run.finish()
    }
    await chat.open()
    await chat.send('x')
    expect(chat.getState().items[1]).toMatchObject({ kind: 'tool', id: 'z', status: 'failed' })
  })

  test('failed tools count in the turn summary: counts, not a list', async () => {
    const { fake, chat } = setup()
    fake.onRun = async (run) => {
      run.start()
      await run.thought('a')
      await run.thought('b')
      await run.tool('Read', { status: 'completed' })
      await run.tool('Edit', { status: 'failed' })
      await run.tool('ToolSearch')
      await run.text('Summary.')
      run.finish()
    }
    await chat.open()
    await chat.send('go')
    const blocks = toBlocks(chat.getState().items)
    expect(blocks.map((b) => b.kind)).toEqual(['user', 'activity', 'reply'])
    expect(blocks[1]).toMatchObject({ reasoning: 2, tools: 2, failed: 1 })
    expect(liveActivityId(blocks, 'idle')).toBeUndefined()
    expect(liveActivityId(blocks, 'running')).toBe(blocks[1].id)
  })

  test('state deltas update the session (usage, commands, mode, plan)', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => {
      run.start()
      run.patch([
        { op: 'replace', path: '/session/usage', value: { used: 5000, size: 200000 } },
        {
          op: 'replace',
          path: '/session/commands',
          value: [{ name: 'init', description: 'Set up' }]
        },
        { op: 'replace', path: '/session/modes/current', value: 'plan' },
        {
          op: 'add',
          path: '/plan',
          value: [{ content: 'Do it', status: 'pending', priority: 'high' }]
        }
      ])
      run.finish()
    }
    await chat.open()
    await chat.send('go')
    const { session, plan } = chat.getState().state
    expect(session.usage).toEqual({ used: 5000, size: 200000 })
    expect(session.commands.map((c) => c.name)).toEqual(['init'])
    expect(session.modes?.current).toBe('plan')
    expect(plan).toHaveLength(1)
  })

  test('a state snapshot replaces the state', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => {
      run.start()
      run.send({
        type: 'STATE_SNAPSHOT',
        snapshot: { session: { title: 'Fresh', config: [] }, plan: null }
      })
      run.finish()
    }
    await chat.open()
    await chat.send('go')
    expect(chat.getState().state.session).toMatchObject({
      title: 'Fresh',
      config: [],
      commands: []
    })
  })

  test('RUN_ERROR becomes an error item and the chat can send again', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => (run.start(), run.error('The agent exited (code 1)', 'agent_exited'))
    await chat.open()
    await chat.send('go')
    expect(chat.getState().status).toBe('error')
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: 'The agent exited (code 1)'
    })
    fake.onRun = async (run) => (run.start(), await run.text('back'), run.finish())
    await chat.send('again')
    expect(chat.getState().status).toBe('idle')
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'back' })
  })

  test('a stream that ends without finishing is an error, not a hang', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => (run.start(), run.close())
    await chat.open()
    await chat.send('go')
    expect(chat.getState().status).toBe('error')
    expect(chat.getState().items.at(-1)).toMatchObject({ kind: 'error' })
  })

  test('a malformed frame is reported', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => (run.start(), run.send({ type: 'NOT_AN_EVENT' }), run.finish())
    await chat.open()
    await chat.send('go')
    expect(chat.getState().status).toBe('error')
  })

  test('an empty message is ignored, and so is a second send while running', async () => {
    const { fake, chat } = setup()
    let release!: () => void
    const gate = new Promise<void>((r) => (release = r))
    fake.onRun = async (run) => (run.start(), await gate, run.finish())
    await chat.open()
    await chat.send('   ')
    expect(fake.callsTo('POST', '/agui')).toHaveLength(0)
    const first = chat.send('one')
    await until(() => chat.getState().status === 'running')
    await chat.send('two')
    expect(fake.callsTo('POST', '/agui')).toHaveLength(1)
    release()
    await first
  })
})

describe('http failures', () => {
  test('409: the host says a run is already going', async () => {
    const { fake, chat } = setup()
    fake.thread('t1').running = true
    await chat.open()
    // the snapshot says running, so force the send path: the host still refuses
    fake.overrides.set('POST /agui', () =>
      Response.json({ error: 'a run is already in progress on this thread' }, { status: 409 })
    )
    ;(chat as unknown as { s: { status: string } }).s.status = 'idle'
    await chat.send('hi')
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: 'a run is already in progress on this thread'
    })
  })

  test('404: an unknown agent is shown with the host message', async () => {
    const { fake, chat } = setup()
    await chat.open()
    fake.overrides.set('POST /agui', () =>
      Response.json({ error: 'no such agent "ghost"' }, { status: 404 })
    )
    await chat.send('hi')
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'no such agent "ghost"' })
    expect(chat.getState().status).toBe('error')
  })

  test('422: a plain-text refusal is shown as is', async () => {
    const { fake, chat } = setup()
    await chat.open()
    fake.overrides.set('POST /agui', () => new Response('invalid RunAgentInput', { status: 422 }))
    await chat.send('hi')
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'invalid RunAgentInput' })
  })

  test('a network failure is an error item', async () => {
    const { fake, chat } = setup()
    await chat.open()
    fake.overrides.set('POST /agui', () => {
      throw new TypeError('fetch failed')
    })
    await chat.send('hi')
    expect(chat.getState().items.at(-1)).toMatchObject({ text: 'fetch failed' })
  })
})

describe('permission prompts (interrupts)', () => {
  const askThenFinish = (
    fake: FakeAgui,
    ask: (run: FakeRun) => void = (run) =>
      run.interrupt({ title: 'mcp__agentos__calculator__show_on_calculator', id: 'int-1' })
  ): void => {
    fake.onRun = async (run) => {
      run.start()
      await run.text('Let me do that.')
      ask(run)
    }
    fake.onResume = async (run) => {
      run.start()
      await run.tool('mcp__agentos__calculator__show_on_calculator')
      await run.text('Done.')
      run.finish()
    }
  }

  test('an interrupt outcome ends the run with a permission prompt and status waiting', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    await chat.open()
    await chat.send('do it')
    const s = chat.getState()
    expect(s.status).toBe('waiting')
    const p = s.items.at(-1)
    expect(p).toMatchObject({
      kind: 'permission',
      interruptId: 'int-1',
      title: 'Calculator · show on calculator'
    })
    expect(p?.kind === 'permission' && p.options.map((o) => o.kind)).toEqual([
      'allow_once',
      'allow_always',
      'reject_once'
    ])
  })

  test('allow once resumes the turn with the chosen option, then finishes', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    await chat.open()
    await chat.send('do it')
    await chat.respond('permission-int-1', 'allow')
    const resume = fake.callsTo('POST', '/agui')[1].body as { resume: unknown; messages: unknown[] }
    expect(resume.resume).toEqual([
      { interruptId: 'int-1', status: 'resolved', payload: { optionId: 'allow' } }
    ])
    expect(resume.messages).toEqual([])
    const s = chat.getState()
    expect(s.status).toBe('idle')
    expect(s.items.find((i) => i.kind === 'permission')).toMatchObject({ answer: 'Allow once' })
    expect(s.items.at(-1)).toMatchObject({ kind: 'agent', text: 'Done.' })
  })

  test('always allow sends that option; deny sends the reject option', async () => {
    for (const [option, answer] of [
      ['allow-always', 'Always allow'],
      ['reject', 'Deny']
    ] as const) {
      const { fake, chat } = setup()
      askThenFinish(fake)
      await chat.open()
      await chat.send('do it')
      await chat.respond('permission-int-1', option)
      const resume = fake.callsTo('POST', '/agui')[1].body as { resume: unknown }
      expect(resume.resume).toEqual([
        { interruptId: 'int-1', status: 'resolved', payload: { optionId: option } }
      ])
      expect(chat.getState().items.find((i) => i.kind === 'permission')).toMatchObject({ answer })
    }
  })

  test('dismissing a prompt (null) resumes as cancelled', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    await chat.open()
    await chat.send('do it')
    await chat.respond('permission-int-1', null)
    const resume = fake.callsTo('POST', '/agui')[1].body as { resume: unknown }
    expect(resume.resume).toEqual([{ interruptId: 'int-1', status: 'cancelled' }])
    expect(chat.getState().items.find((i) => i.kind === 'permission')).toMatchObject({
      answer: 'Denied'
    })
  })

  test('an agent that asks twice with the same interrupt id is shown one prompt (the transcript is keyed by id)', async () => {
    const { fake, chat } = setup()
    const ask = { id: 'dup', reason: 'tool_call', message: 'Run it' }
    fake.onRun = (run) => {
      run.start()
      run.send({
        type: 'RUN_FINISHED',
        threadId: 't1',
        runId: run.input.runId,
        outcome: { type: 'interrupt', interrupts: [ask, ask] }
      })
      run.close()
    }
    await chat.open()
    await chat.send('go')
    const ids = chat.getState().items.map((i) => i.id)
    expect(ids.filter((id) => id === 'permission-dup')).toHaveLength(1)
    expect(new Set(ids).size).toBe(ids.length)
  })

  test('history with repeated message ids still gets an id per item', () => {
    const items = itemsFromMessages([
      { id: 'm', role: 'user', content: 'one' },
      { id: 'm', role: 'user', content: 'two' },
      { id: 'm', role: 'assistant', content: 'three' }
    ])
    const ids = items.map((i) => i.id)
    expect(new Set(ids).size).toBe(3)
    expect(ids[0]).toBe('m')
    expect(items.map((i) => i.kind === 'user' && i.text)).toEqual(['one', 'two', false])
    expect(new Set(toBlocks(items).map((b) => b.id)).size).toBe(toBlocks(items).length)
  })

  test('a resume must cover every open prompt: it waits for the last answer', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => {
      run.start()
      run.send({
        type: 'RUN_FINISHED',
        threadId: 't1',
        runId: run.input.runId,
        outcome: {
          type: 'interrupt',
          interrupts: [
            {
              id: 'a',
              reason: 'tool_call',
              message: 'One',
              metadata: {
                'agentos:options': [{ optionId: 'ok', name: 'Allow', kind: 'allow_once' }]
              }
            },
            {
              id: 'b',
              reason: 'tool_call',
              message: 'Two',
              metadata: {
                'agentos:options': [{ optionId: 'ok', name: 'Allow', kind: 'allow_once' }]
              }
            }
          ]
        }
      })
      run.close()
    }
    fake.onResume = (run) => (run.start(), run.finish())
    await chat.open()
    await chat.send('go')
    await chat.respond('permission-a', 'ok')
    expect(fake.callsTo('POST', '/agui')).toHaveLength(1)
    expect(chat.getState().status).toBe('waiting')
    await chat.respond('permission-b', 'ok')
    const resume = (fake.callsTo('POST', '/agui')[1].body as { resume: { interruptId: string }[] })
      .resume
    expect(resume.map((r) => r.interruptId).sort()).toEqual(['a', 'b'])
  })

  test('a prompt can only be answered once', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    await chat.open()
    await chat.send('do it')
    await chat.respond('permission-int-1', 'allow')
    await chat.respond('permission-int-1', 'reject')
    expect(fake.callsTo('POST', '/agui')).toHaveLength(2)
  })

  test('no new message while a prompt is open', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    await chat.open()
    await chat.send('do it')
    await chat.send('and another thing')
    expect(fake.callsTo('POST', '/agui')).toHaveLength(1)
  })

  test('the resume run can ask again', async () => {
    const { fake, chat } = setup()
    askThenFinish(fake)
    fake.onResume = (run) => (run.start(), run.interrupt({ title: 'Bash', id: 'int-2' }))
    await chat.open()
    await chat.send('do it')
    await chat.respond('permission-int-1', 'allow')
    expect(chat.getState().status).toBe('waiting')
    expect(chat.getState().items.filter((i) => i.kind === 'permission')).toHaveLength(2)
  })
})

describe('stop', () => {
  test('aborts the stream, posts cancel, and goes idle', async () => {
    const { fake, chat } = setup()
    fake.onRun = async (run) => {
      run.start()
      run.send({ type: 'TEXT_MESSAGE_START', messageId: 'm', role: 'assistant' })
      run.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: 'Working' })
      await run.wait(5000)
    }
    await chat.open()
    const sent = chat.send('long job')
    await until(() => chat.getState().items.some((i) => i.kind === 'agent' && i.text === 'Working'))
    await chat.stop()
    await sent
    expect(chat.getState().status).toBe('idle')
    expect(chat.getState().items.at(-1)).toMatchObject({ kind: 'agent', streaming: false })
    expect(fake.callsTo('POST', '/agui/threads/t1/cancel')).toHaveLength(1)
    expect(fake.runs[0].signal.aborted).toBe(true)
    // no error item for a stop the person asked for
    expect(kinds(chat)).not.toContain('error')
  })

  test('stopping while a prompt is open cancels it', async () => {
    const { fake, chat } = setup()
    fake.onRun = (run) => (run.start(), run.interrupt({ title: 'Bash', id: 'i' }))
    await chat.open()
    await chat.send('go')
    await chat.stop()
    expect(chat.getState().status).toBe('idle')
    expect(chat.getState().items.at(-1)).toMatchObject({ kind: 'permission', answer: 'Cancelled' })
    expect(fake.callsTo('POST', '/agui/threads/t1/cancel')).toHaveLength(1)
  })

  test('stop does nothing when idle', async () => {
    const { fake, chat } = setup()
    await chat.open()
    await chat.stop()
    expect(fake.callsTo('POST', '/agui/threads/t1/cancel')).toHaveLength(0)
  })

  test('dispose drops the stream without cancelling the turn or notifying listeners', async () => {
    const { fake, chat } = setup()
    fake.onRun = async (run) => (run.start(), await run.wait(5000))
    await chat.open()
    let notified = 0
    chat.subscribe(() => notified++)
    void chat.send('x')
    await until(() => fake.runs.length === 1)
    chat.dispose()
    const before = notified
    await new Promise((r) => setTimeout(r, 20))
    expect(fake.runs[0].signal.aborted).toBe(true)
    expect(notified).toBe(before)
    expect(fake.callsTo('POST', '/agui/threads/t1/cancel')).toHaveLength(0)
  })
})

describe('session settings', () => {
  test('setMode applies at once and adopts the host state', async () => {
    const { fake, chat } = setup()
    await chat.open()
    const done = chat.setMode('plan')
    expect(chat.getState().state.session.modes?.current).toBe('plan')
    await done
    expect(fake.callsTo('POST', '/agui/threads/t1/settings')[0].body).toEqual({ mode: 'plan' })
    expect(chat.getState().state.session.modes?.current).toBe('plan')
  })

  test('setConfig sends the option id and value, select or boolean', async () => {
    const { fake, chat } = setup()
    await chat.open()
    await chat.setConfig('model', 'small')
    await chat.setConfig('fast', true)
    const calls = fake.callsTo('POST', '/agui/threads/t1/settings').map((c) => c.body)
    expect(calls).toEqual([
      { config: { id: 'model', value: 'small' } },
      { config: { id: 'fast', value: true } }
    ])
    const config = chat.getState().state.session.config
    expect(config.find((c) => c.id === 'model')?.current).toBe('small')
    expect(config.find((c) => c.id === 'fast')?.current).toBe(true)
  })

  test('a refused change is rolled back and reported', async () => {
    const { chat } = setup()
    await chat.open()
    await chat.setMode('nonsense')
    expect(chat.getState().state.session.modes?.current).toBe('default')
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: 'unknown mode nonsense'
    })
    expect(chat.getState().status).toBe('idle')
  })
})

describe('pieces', () => {
  test('friendlyTool shortens AgentOS MCP tool names and leaves the rest', () => {
    expect(friendlyTool('mcp__agentos__calculator__show_on_calculator')).toBe(
      'Calculator · show on calculator'
    )
    expect(friendlyTool('Bash')).toBe('Bash')
  })

  test('itemsFromMessages skips tool results and unknown roles', () => {
    expect(
      itemsFromMessages([
        { id: 't', role: 'tool', content: 'x' },
        { id: 'd', role: 'developer', content: 'x' },
        {
          id: 'u',
          role: 'user',
          content: [
            { type: 'text', text: 'hi ' },
            { type: 'text', text: 'there' }
          ]
        }
      ]).map((i) => i.kind === 'user' && i.text)
    ).toEqual(['hi there'])
  })

  test('consecutive agent messages merge into one reply per turn', () => {
    const blocks = toBlocks([
      { kind: 'user', id: 'u', text: 'q' },
      { kind: 'agent', id: 'a1', text: 'one', streaming: false },
      { kind: 'tool', id: 't', name: 'Read', title: 'Read', status: 'completed' },
      { kind: 'agent', id: 'a2', text: 'two', streaming: false },
      { kind: 'user', id: 'u2', text: 'q2' },
      { kind: 'agent', id: 'a3', text: 'three', streaming: false }
    ])
    expect(
      blocks.filter((b) => b.kind === 'reply').map((b) => b.kind === 'reply' && b.text)
    ).toEqual(['one\n\ntwo', 'three'])
    expect(blocks.filter((b) => b.kind === 'activity')).toHaveLength(2)
  })
})
