import { describe, expect, test } from 'bun:test'
import { DefinitionError, parseDefinition } from './definition.ts'

const doc = (overrides: Record<string, unknown> = {}): string =>
  JSON.stringify({
    openrpc: '1.3.2',
    info: { title: 'Notes', version: '1.0.0' },
    'x-app': { id: 'notes', icon: 'doc', window: { w: 480, h: 640, minW: 320, minH: 320 } },
    methods: [
      {
        name: 'add',
        params: [{ name: 'title', required: true, schema: { type: 'string', minLength: 1 } }],
        result: { name: 'note', schema: { $ref: '#/components/schemas/Note' } }
      }
    ],
    components: {
      schemas: {
        Note: {
          type: 'object',
          properties: { id: { type: 'integer' }, title: { type: 'string' }, tag: { type: ['string', 'null'] } },
          required: ['id', 'title']
        }
      }
    },
    ...overrides
  })

const problem = (source: string): string => {
  try {
    parseDefinition(source)
  } catch (e) {
    if (e instanceof DefinitionError) return e.message
    throw e
  }
  return ''
}

describe('parseDefinition', () => {
  test('reads methods, params, results and the components they refer to', () => {
    const d = parseDefinition(doc())
    expect(d.title).toBe('Notes')
    expect(d.app).toEqual({ id: 'notes', icon: 'doc', window: { w: 480, h: 640, minW: 320, minH: 320 } })
    expect(d.methods.map((m) => m.name)).toEqual(['add'])
    expect(d.methods[0]?.params[0]).toEqual({
      name: 'title',
      required: true,
      schema: { kind: 'string', minLength: 1 }
    })
    expect(d.methods[0]?.result).toEqual({ kind: 'ref', name: 'Note' })
    expect(d.components.Note).toMatchObject({ kind: 'object', required: ['id', 'title'] })
    // a type with null is a nullable of the other
    const note = d.components.Note
    expect(note?.kind === 'object' && note.properties.tag).toEqual({ kind: 'nullable', inner: { kind: 'string' } })
  })

  test('says where the problem is', () => {
    expect(problem('{ nope')).toContain('not JSON')
    expect(problem(doc({ openrpc: '2.0.0' }))).toContain('openrpc: version 2.0.0 is not supported')
    expect(problem(doc({ info: {} }))).toContain('info.title')
    expect(problem(doc({ methods: 'x' }))).toContain('methods: must be a list')
    expect(problem(doc({ methods: [{ name: 'add', params: [], result: { name: 'r', schema: { type: 'strng' } } }] }))).toContain(
      'methods[0].result.schema.type: `strng` is not supported'
    )
  })

  test('the app\'s metadata is part of the contract: x-app names it, and a version is major.minor.patch', () => {
    expect(problem(doc({ 'x-app': undefined }))).toContain('x-app: is required')
    expect(problem(doc({ 'x-app': { id: 'No' } }))).toContain('x-app.id: must be 3 to 40 characters')
    expect(problem(doc({ 'x-app': { id: 'notes', colour: 'red' } }))).toContain('x-app.colour: is not supported')
    expect(problem(doc({ 'x-app': { id: 'notes', window: { w: 1, h: 1, minW: 0, minH: 1 } } }))).toContain('x-app.window.minW')
    expect(problem(doc({ info: { title: 'Notes', version: '1' } }))).toContain('info.version: must be major.minor.patch')
  })

  test('a method is an identifier, defined once, with its params by name and a result', () => {
    const method = (m: Record<string, unknown>): string =>
      doc({ methods: [{ params: [], result: { name: 'r', schema: { type: 'null' } }, ...m }] })
    expect(problem(method({ name: 'add-task' }))).toContain('must be an identifier')
    expect(problem(method({ name: 'end' }))).toContain('reserved word')
    expect(problem(method({ name: 'a', paramStructure: 'by-position' }))).toContain('must be "by-name"')
    expect(problem(method({ name: 'a', result: undefined }))).toContain('needs a `result`')
    const twice = doc({
      methods: [
        { name: 'a', params: [], result: { name: 'r', schema: { type: 'null' } } },
        { name: 'a', params: [], result: { name: 'r', schema: { type: 'null' } } }
      ]
    })
    expect(problem(twice)).toContain('method `a` is defined twice')
    expect(problem(method({ name: 'a', params: [{ name: 'x', schema: { type: 'null' } }, { name: 'x', schema: { type: 'null' } }] }))).toContain(
      'param `x` is given twice'
    )
  })

  test('refuses what the subset does not carry, and a $ref that points nowhere', () => {
    const withSchema = (schema: unknown): string =>
      doc({ methods: [{ name: 'a', params: [{ name: 'p', schema }], result: { name: 'r', schema: { type: 'null' } } }] })
    expect(problem(withSchema({ type: 'string', pattern: '^a' }))).toContain('.pattern: is not supported')
    expect(problem(withSchema({ anyOf: [{ type: 'string' }] }))).toContain('.type: must be a type name')
    expect(problem(withSchema({ $ref: '#/components/schemas/Missing' }))).toContain('refers to `Missing`')
    expect(problem(withSchema({ $ref: 'http://elsewhere/x' }))).toContain('must be "#/components/schemas/<Name>"')
    expect(problem(withSchema({ type: 'array' }))).toContain('needs `items`')
    expect(problem(withSchema({ type: 'object', properties: { a: { type: 'null' } }, required: ['b'] }))).toContain(
      'names `b`, which is not a property'
    )
    expect(problem(withSchema({ type: 'string', enum: [] }))).toContain('non-empty list of strings')
    expect(problem(withSchema({ type: 'integer', minimum: 'one' }))).toContain('.minimum: must be a number')
  })
})
