import { describe, expect, test } from 'bun:test'
import { ChatSession } from './session'
import { hostTransport, type ChatTransport, type EventBatch, type HostCall } from './transport'

const snapshot = {
  threadId: 't1',
  agentId: 'scripted',
  running: false,
  messages: [],
  interrupts: []
}

/** A transport whose every method a test can replace. */
function transport(over: Partial<ChatTransport> = {}): ChatTransport & { calls: string[] } {
  const calls: string[] = []
  const log =
    <A extends unknown[], R>(name: string, fn: (...a: A) => Promise<R>) =>
    (...a: A): Promise<R> => {
      calls.push(name)
      return fn(...a)
    }
  return {
    calls,
    thread: log('thread', over.thread ?? (async () => snapshot)),
    run: log('run', over.run ?? (async () => {})),
    events: log(
      'events',
      over.events ?? (async (): Promise<EventBatch> => ({ events: [], last: 0, done: true }))
    ),
    cancel: log('cancel', over.cancel ?? (async () => {})),
    detach: log('detach', over.detach ?? (async () => {})),
    settings: log('settings', over.settings ?? (async () => ({})))
  }
}

const session = (t: ChatTransport): ChatSession =>
  new ChatSession({ threadId: 't1', agentId: 'scripted', transport: t, newId: () => 'id' })

describe('the chat over the host page', () => {
  test('a refusal from the host page is shown to the person as it was said', async () => {
    const chat = session(
      transport({
        run: async () => {
          throw new Error(
            '4 chats are already answering in this window: stop one, or wait for one to finish'
          )
        }
      })
    )
    await chat.send('hi')
    expect(chat.getState().status).toBe('error')
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: '4 chats are already answering in this window: stop one, or wait for one to finish'
    })
  })

  test('events are pulled from where the last pull ended until the run is done', async () => {
    const afters: number[] = []
    const batches: EventBatch[] = [
      { events: [{ type: 'RUN_STARTED', threadId: 't1', runId: 'id' }], last: 1, done: false },
      { events: [], last: 1, done: false }, // an idle answer
      { events: [{ type: 'RUN_FINISHED', threadId: 't1', runId: 'id' }], last: 2, done: true }
    ]
    const chat = session(
      transport({
        events: async (_t, _r, after) => {
          afters.push(after)
          return batches.shift() ?? { events: [], last: after, done: true }
        }
      })
    )
    await chat.send('hi')
    expect(afters).toEqual([0, 1, 1])
    expect(chat.getState().status).toBe('idle')
  })

  test('a pull that fails ends the run with the reason', async () => {
    const chat = session(
      transport({
        events: async () => {
          throw new Error('Could not reach vos: Load failed')
        }
      })
    )
    await chat.send('hi')
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: 'Could not reach vos: Load failed'
    })
  })

  test('closing the chat mid-run detaches it; stopping it cancels the turn', async () => {
    let release!: () => void
    const waiting = new Promise<EventBatch>((resolve) => {
      release = () => resolve({ events: [], last: 0, done: true })
    })
    const t = transport({ events: () => waiting })
    const chat = session(t)
    void chat.send('hi')
    await new Promise((r) => setTimeout(r, 0))
    chat.dispose()
    release()
    expect(t.calls).toContain('detach')
    expect(t.calls).not.toContain('cancel')

    const t2 = transport({ events: () => new Promise(() => {}) })
    const other = session(t2)
    void other.send('hi')
    await new Promise((r) => setTimeout(r, 0))
    await other.stop()
    expect(t2.calls).toContain('cancel')
  })

  test('a stop the host page refuses is said in the transcript', async () => {
    const chat = session(
      transport({
        events: () => new Promise(() => {}),
        cancel: async () => {
          throw new Error('no thread `agents:t1`')
        }
      })
    )
    void chat.send('hi')
    await new Promise((r) => setTimeout(r, 0))
    await chat.stop()
    expect(chat.getState().items.at(-1)).toMatchObject({
      kind: 'error',
      text: 'Could not stop the agent: no thread `agents:t1`'
    })
  })
})

describe('hostTransport', () => {
  test('each method is the host service of the same name, with the page’s own ids', async () => {
    const sent: [string, unknown][] = []
    const call: HostCall = async (method, params) => {
      sent.push([method, params])
      return method === 'vos/agui/events' ? { events: [], last: 0, done: true } : {}
    }
    const t = hostTransport(call)
    await t.thread('t1', 'a')
    await t.run({
      threadId: 't1',
      runId: 'r',
      agentId: 'a',
      messages: [{ id: 'm', role: 'user', content: 'x' }]
    })
    await t.events('t1', 'r', 3)
    await t.cancel('t1')
    await t.detach('t1')
    await t.settings('t1', { mode: 'plan' })
    expect(sent).toEqual([
      ['vos/agui/thread', { threadId: 't1', agentId: 'a' }],
      [
        'vos/agui/run',
        {
          threadId: 't1',
          runId: 'r',
          agentId: 'a',
          messages: [{ id: 'm', role: 'user', content: 'x' }]
        }
      ],
      ['vos/agui/events', { threadId: 't1', runId: 'r', after: 3 }],
      ['vos/agui/cancel', { threadId: 't1' }],
      ['vos/agui/detach', { threadId: 't1' }],
      ['vos/agui/settings', { threadId: 't1', change: { mode: 'plan' } }]
    ])
  })

  test('a pull answered with something else is an error, not a hang', async () => {
    const t = hostTransport(async () => ({ nope: true }))
    expect(t.events('t1', 'r', 0)).rejects.toThrow(
      'The host page answered a pull of the run with something else'
    )
  })
})
