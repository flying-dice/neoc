/**
 * lib:agui against aimock's AG-UI agent: a script (events.luau) starts a run through the real `neoc`, and this
 * checks what it was given. Every event type of the protocol goes through once, with every field it can carry, and
 * must arrive under the Python SDK's names with the data inside it untouched.
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { resolve } from 'node:path'
import { AGUIMock, type AGUIEvent, type AGUIRunAgentInput } from '@copilotkit/aimock'

const ROOT = resolve(import.meta.dir, '../../..')

/** Every event type: what the agent sends (the wire, camelCase), and what a script must be given for it. */
const EVENTS: Array<[sent: AGUIEvent, given: Record<string, unknown>]> = [
  [
    { type: 'RUN_STARTED', threadId: 'thread-1', runId: 'run-1', parentRunId: 'run-0', protocolVersion: '1.0', timestamp: 1700000000000 },
    { type: 'RUN_STARTED', thread_id: 'thread-1', run_id: 'run-1', parent_run_id: 'run-0', protocol_version: '1.0', timestamp: 1700000000000 }
  ],
  [{ type: 'STEP_STARTED', stepName: 'plan' }, { type: 'STEP_STARTED', step_name: 'plan' }],
  [
    { type: 'TEXT_MESSAGE_START', messageId: 'm1', role: 'assistant', name: 'Ada' },
    { type: 'TEXT_MESSAGE_START', message_id: 'm1', role: 'assistant', name: 'Ada' }
  ],
  [
    { type: 'TEXT_MESSAGE_CONTENT', messageId: 'm1', delta: 'Hello' },
    { type: 'TEXT_MESSAGE_CONTENT', message_id: 'm1', delta: 'Hello' }
  ],
  [{ type: 'TEXT_MESSAGE_END', messageId: 'm1' }, { type: 'TEXT_MESSAGE_END', message_id: 'm1' }],
  [
    { type: 'TEXT_MESSAGE_CHUNK', messageId: 'm2', role: 'assistant', delta: 'in one' },
    { type: 'TEXT_MESSAGE_CHUNK', message_id: 'm2', role: 'assistant', delta: 'in one' }
  ],
  [
    { type: 'TOOL_CALL_START', toolCallId: 'c1', toolCallName: 'notes__add', parentMessageId: 'm1' },
    { type: 'TOOL_CALL_START', tool_call_id: 'c1', tool_call_name: 'notes__add', parent_message_id: 'm1' }
  ],
  [
    { type: 'TOOL_CALL_ARGS', toolCallId: 'c1', delta: '{"noteTitle":"x"}' },
    { type: 'TOOL_CALL_ARGS', tool_call_id: 'c1', delta: '{"noteTitle":"x"}' }
  ],
  [{ type: 'TOOL_CALL_END', toolCallId: 'c1' }, { type: 'TOOL_CALL_END', tool_call_id: 'c1' }],
  [
    { type: 'TOOL_CALL_CHUNK', toolCallId: 'c2', toolCallName: 'notes__list', parentMessageId: 'm1', delta: '{}' },
    { type: 'TOOL_CALL_CHUNK', tool_call_id: 'c2', tool_call_name: 'notes__list', parent_message_id: 'm1', delta: '{}' }
  ],
  [
    { type: 'TOOL_CALL_RESULT', messageId: 'm3', toolCallId: 'c1', content: 'added', role: 'tool' },
    { type: 'TOOL_CALL_RESULT', message_id: 'm3', tool_call_id: 'c1', content: 'added', role: 'tool' }
  ],
  [
    { type: 'STATE_SNAPSHOT', snapshot: { openNotes: ['a'], perUser: { firstName: 'Ada' } } },
    { type: 'STATE_SNAPSHOT', snapshot: { openNotes: ['a'], perUser: { firstName: 'Ada' } } }
  ],
  [
    { type: 'STATE_DELTA', delta: [{ op: 'add', path: '/openNotes/-', value: 'b' }] },
    { type: 'STATE_DELTA', delta: [{ op: 'add', path: '/openNotes/-', value: 'b' }] }
  ],
  [
    {
      type: 'MESSAGES_SNAPSHOT',
      messages: [
        { id: 'u1', role: 'user', content: 'hi' },
        { id: 'm1', role: 'assistant', content: 'Hello', toolCalls: [{ id: 'c1', type: 'function', function: { name: 'notes__add', arguments: '{}' } }] },
        { id: 'm3', role: 'tool', content: 'added', toolCallId: 'c1' }
      ]
    },
    {
      type: 'MESSAGES_SNAPSHOT',
      messages: [
        { id: 'u1', role: 'user', content: 'hi' },
        { id: 'm1', role: 'assistant', content: 'Hello', tool_calls: [{ id: 'c1', type: 'function', function: { name: 'notes__add', arguments: '{}' } }] },
        { id: 'm3', role: 'tool', content: 'added', tool_call_id: 'c1' }
      ]
    }
  ],
  [
    { type: 'ACTIVITY_SNAPSHOT', messageId: 'a1', activityType: 'a2ui-surface', content: { a2ui_operations: [{ createSurface: { surfaceId: 's1' } }] }, replace: false },
    { type: 'ACTIVITY_SNAPSHOT', message_id: 'a1', activity_type: 'a2ui-surface', content: { a2ui_operations: [{ createSurface: { surfaceId: 's1' } }] }, replace: false }
  ],
  [
    { type: 'ACTIVITY_DELTA', messageId: 'a1', activityType: 'a2ui-surface', patch: [{ op: 'replace', path: '/a2ui_operations/0/createSurface/surfaceId', value: 's2' }] },
    { type: 'ACTIVITY_DELTA', message_id: 'a1', activity_type: 'a2ui-surface', patch: [{ op: 'replace', path: '/a2ui_operations/0/createSurface/surfaceId', value: 's2' }] }
  ],
  [{ type: 'REASONING_START', messageId: 'r1' }, { type: 'REASONING_START', message_id: 'r1' }],
  [
    { type: 'REASONING_MESSAGE_START', messageId: 'r1', role: 'reasoning' },
    { type: 'REASONING_MESSAGE_START', message_id: 'r1', role: 'reasoning' }
  ],
  [
    { type: 'REASONING_MESSAGE_CONTENT', messageId: 'r1', delta: 'thinking' },
    { type: 'REASONING_MESSAGE_CONTENT', message_id: 'r1', delta: 'thinking' }
  ],
  [{ type: 'REASONING_MESSAGE_END', messageId: 'r1' }, { type: 'REASONING_MESSAGE_END', message_id: 'r1' }],
  [
    { type: 'REASONING_MESSAGE_CHUNK', messageId: 'r2', delta: 'more' },
    { type: 'REASONING_MESSAGE_CHUNK', message_id: 'r2', delta: 'more' }
  ],
  [{ type: 'REASONING_END', messageId: 'r1' }, { type: 'REASONING_END', message_id: 'r1' }],
  [
    { type: 'REASONING_ENCRYPTED_VALUE', subtype: 'message', entityId: 'r1', encryptedValue: 'b64==' },
    { type: 'REASONING_ENCRYPTED_VALUE', subtype: 'message', entity_id: 'r1', encrypted_value: 'b64==' }
  ],
  [
    { type: 'SUBAGENT_STARTED', subagentRunId: 's1', name: 'researcher', description: 'Reads', parentSubagentRunId: 's0', parentToolCallId: 'c1', parentMessageId: 'm1' },
    { type: 'SUBAGENT_STARTED', subagent_run_id: 's1', name: 'researcher', description: 'Reads', parent_subagent_run_id: 's0', parent_tool_call_id: 'c1', parent_message_id: 'm1' }
  ],
  [
    { type: 'SUBAGENT_FINISHED', subagentRunId: 's1', result: { ok: true }, outcome: { type: 'suspended', interruptIds: ['i1'] } },
    { type: 'SUBAGENT_FINISHED', subagent_run_id: 's1', result: { ok: true }, outcome: { type: 'suspended', interrupt_ids: ['i1'] } }
  ],
  [
    { type: 'SUBAGENT_ERROR', subagentRunId: 's2', message: 'lost', code: 'E1' },
    { type: 'SUBAGENT_ERROR', subagent_run_id: 's2', message: 'lost', code: 'E1' }
  ],
  [
    { type: 'RAW', event: { someVendor: { eventKind: 'x' } }, source: 'langgraph' },
    { type: 'RAW', event: { someVendor: { eventKind: 'x' } }, source: 'langgraph' }
  ],
  [
    { type: 'CUSTOM', name: 'confetti', value: { howMuch: 3 }, rawEvent: { fromVendor: true }, metadata: { traceId: 't' } },
    { type: 'CUSTOM', name: 'confetti', value: { howMuch: 3 }, raw_event: { fromVendor: true }, metadata: { traceId: 't' } }
  ],
  [{ type: 'THINKING_START', title: 'Planning' }, { type: 'THINKING_START', title: 'Planning' }],
  [{ type: 'THINKING_TEXT_MESSAGE_START' }, { type: 'THINKING_TEXT_MESSAGE_START' }],
  [{ type: 'THINKING_TEXT_MESSAGE_CONTENT', delta: 'hm' }, { type: 'THINKING_TEXT_MESSAGE_CONTENT', delta: 'hm' }],
  [{ type: 'THINKING_TEXT_MESSAGE_END' }, { type: 'THINKING_TEXT_MESSAGE_END' }],
  [{ type: 'THINKING_END' }, { type: 'THINKING_END' }],
  [{ type: 'STEP_FINISHED', stepName: 'plan' }, { type: 'STEP_FINISHED', step_name: 'plan' }],
  [
    { type: 'RUN_ERROR', message: 'the model refused', code: 'refused', usage: [{ model: 'm', inputTokens: 1 }] },
    { type: 'RUN_ERROR', message: 'the model refused', code: 'refused', usage: [{ model: 'm', input_tokens: 1 }] }
  ],
  [
    {
      type: 'RUN_FINISHED',
      threadId: 'thread-1',
      runId: 'run-1',
      result: { finalAnswer: 42 },
      outcome: { type: 'interrupt', interrupts: [{ id: 'i1', reason: 'tool_approval', message: 'Add the note?', toolCallId: 'c1', responseSchema: { type: 'boolean' }, expiresAt: '2030-01-01T00:00:00Z' }] },
      usage: [{ provider: 'p', model: 'm', inputTokens: 3, outputTokens: 5, totalTokens: 8, reasoningTokens: 1, cachedInputTokens: 2 }]
    },
    {
      type: 'RUN_FINISHED',
      thread_id: 'thread-1',
      run_id: 'run-1',
      result: { finalAnswer: 42 },
      outcome: { type: 'interrupt', interrupts: [{ id: 'i1', reason: 'tool_approval', message: 'Add the note?', tool_call_id: 'c1', response_schema: { type: 'boolean' }, expires_at: '2030-01-01T00:00:00Z' }] },
      usage: [{ provider: 'p', model: 'm', input_tokens: 3, output_tokens: 5, total_tokens: 8, reasoning_tokens: 1, cached_input_tokens: 2 }]
    }
  ]
]

const agent = new AGUIMock()
const inputs: AGUIRunAgentInput[] = []
let url = ''

beforeAll(async () => {
  agent.onPredicate(
    (input) => {
      inputs.push(input)
      return JSON.stringify(input.messages).includes('every event')
    },
    EVENTS.map(([sent]) => sent)
  )
  agent.onMessage('hello', 'Hi there')
  agent.onToolCall('add a note', 'notes__add', '{"title":"x"}', { result: 'added' })
  agent.onReasoning('think', 'Let me see')
  url = await agent.start()
})
afterAll(() => agent.stop())

/** What a script is given for a run in which the person says `say`: its events, or the error line. */
async function run(say: string): Promise<Array<Record<string, unknown>>> {
  const proc = Bun.spawn(['cargo', 'run', '-q', '-p', 'neoc', '--', 'run', resolve(import.meta.dir, 'events.luau')], {
    cwd: ROOT,
    env: { ...process.env, AGUI_URL: url, AGUI_SAY: say },
    stdout: 'pipe',
    stderr: 'pipe'
  })
  const [out, err, code] = await Promise.all([new Response(proc.stdout).text(), new Response(proc.stderr).text(), proc.exited])
  if (code !== 0) throw new Error(`neoc run failed (${code}): ${err}`)
  return out
    .split('\n')
    .filter((line) => line !== '')
    .map((line) => {
      if (line.startsWith('ERROR ')) throw new Error(line)
      return JSON.parse(line) as Record<string, unknown>
    })
}

const types = (events: Array<Record<string, unknown>>): unknown[] => events.map((e) => e.type)

describe('lib:agui against aimock', () => {
  test('there is an event here for every type the protocol has', () => {
    expect(new Set(EVENTS.map(([sent]) => sent.type)).size).toBe(36)
  })

  test('every event type arrives, each field under its Python name, the data inside as it was sent', async () => {
    const given = await run('every event')
    expect(types(given)).toEqual(EVENTS.map(([sent]) => sent.type))
    for (const [i, [, expected]] of EVENTS.entries()) {
      // aimock stamps the events that have no time of their own
      const { timestamp, ...rest } = given[i] ?? {}
      expect('timestamp' in expected ? { ...rest, timestamp } : rest).toEqual(expected)
    }
  }, 600_000)

  test('the agent is sent the input the protocol describes', async () => {
    inputs.length = 0
    await run('every event')
    expect(inputs[0]).toEqual({
      threadId: 'thread-1',
      runId: 'run-1',
      state: { count: 1 },
      messages: [{ id: 'u1', role: 'user', content: 'every event' }],
      tools: [{ name: 'notes__add', description: 'Add a note', parameters: { type: 'object' } }],
      context: [],
      forwardedProps: {}
    })
  }, 600_000)

  test("aimock's own text, tool call and reasoning runs are read from start to finish", async () => {
    const text = await run('hello')
    expect(types(text)).toEqual(['RUN_STARTED', 'TEXT_MESSAGE_START', 'TEXT_MESSAGE_CONTENT', 'TEXT_MESSAGE_END', 'RUN_FINISHED'])
    expect(text[2]?.delta).toBe('Hi there')

    const tool = await run('add a note')
    expect(types(tool)).toEqual(['RUN_STARTED', 'TOOL_CALL_START', 'TOOL_CALL_ARGS', 'TOOL_CALL_END', 'TOOL_CALL_RESULT', 'RUN_FINISHED'])
    expect(tool[1]?.tool_call_name).toBe('notes__add')
    expect(tool[4]?.content).toBe('added')

    const reasoning = await run('think')
    expect(types(reasoning).filter((t) => String(t).startsWith('REASONING'))).toEqual([
      'REASONING_START',
      'REASONING_MESSAGE_START',
      'REASONING_MESSAGE_CONTENT',
      'REASONING_MESSAGE_END',
      'REASONING_END'
    ])
  }, 600_000)

  test('a run nothing matches is a status error', async () => {
    expect(run('nothing matches this')).rejects.toThrow('agui.run: status: the endpoint answered 404')
  }, 600_000)
})
