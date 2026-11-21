import { describe, expect, test } from 'bun:test'
import { parseThreadState, permissionOf, snapshotSchema, uniqueBy } from './schema'

describe('uniqueBy', () => {
  test('keeps the first of every key, in order', () => {
    const unique = uniqueBy((x: { k: string; v: number }) => x.k)
    expect(
      unique([
        { k: 'a', v: 1 },
        { k: 'b', v: 2 },
        { k: 'a', v: 3 }
      ])
    ).toEqual([
      { k: 'a', v: 1 },
      { k: 'b', v: 2 }
    ])
  })
})

describe('lists the UI keys by an agent-chosen id are deduplicated at the edge', () => {
  test('commands by name, config by id, select options by value, modes by id', () => {
    const state = parseThreadState({
      session: {
        modes: {
          current: 'a',
          available: [
            { id: 'a', name: 'A' },
            { id: 'a', name: 'A again' }
          ]
        },
        config: [
          {
            id: 'model',
            name: 'Model',
            kind: 'select',
            current: 'x',
            options: [
              { value: 'x', name: 'X' },
              { value: 'x', name: 'X again' },
              { value: 'y', name: 'Y' }
            ]
          },
          { id: 'model', name: 'Model twice', kind: 'boolean', current: true },
          { id: 'fast', name: 'Fast', kind: 'boolean', current: false }
        ],
        commands: [
          { name: 'plan', description: 'one' },
          { name: 'plan', description: 'two' },
          { name: 'init' }
        ]
      }
    })
    expect(state.session.modes?.available.map((m) => m.name)).toEqual(['A'])
    expect(state.session.config.map((c) => c.id)).toEqual(['model', 'fast'])
    const model = state.session.config[0]
    expect(model.kind === 'select' && model.options.map((o) => o.value)).toEqual(['x', 'y'])
    expect(state.session.commands.map((c) => c.description)).toEqual(['one', ''])
  })

  test('permission options by optionId', () => {
    const permission = permissionOf({
      id: 'i',
      reason: 'tool_call',
      metadata: {
        'vos:options': [
          { optionId: 'ok', name: 'Allow', kind: 'allow_once' },
          { optionId: 'ok', name: 'Allow (again)', kind: 'allow_always' },
          { optionId: 'no', name: 'Deny', kind: 'reject_once' }
        ]
      }
    })
    expect(permission.options.map((o) => o.name)).toEqual(['Allow', 'Deny'])
  })

  test('interrupts of a snapshot by id', () => {
    const snap = snapshotSchema.parse({
      threadId: 't',
      agentId: 'a',
      interrupts: [
        { id: 'x', reason: 'tool_call' },
        { id: 'x', reason: 'tool_call' }
      ]
    })
    expect(snap.interrupts).toHaveLength(1)
  })
})
