import { describe, expect, test } from 'bun:test'
import { DefinitionError, jsonErrorOffset, parseDefinition } from './definition.ts'

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

  test('x-app carries the frame\'s csp and permissions in the shape of MCP Apps, and refuses what opens everything', () => {
    const ok = parseDefinition(
      doc({
        'x-app': {
          id: 'feeds',
          csp: { resourceDomains: ['https://cdn.example.com', 'https://*.images.example.org:8443'], connectDomains: ['https://api.example.com'] },
          permissions: { clipboardWrite: {} }
        }
      })
    )
    expect(ok.app.csp?.resourceDomains).toEqual(['https://cdn.example.com', 'https://*.images.example.org:8443'])
    expect(ok.app.permissions).toEqual({ clipboardWrite: {} })
    const csp = (resourceDomains: unknown): string => problem(doc({ 'x-app': { id: 'feeds', csp: { resourceDomains } } }))
    for (const bad of ['*', 'https:', 'https://*', "'unsafe-inline'", 'https://*.com', 'ftp://x.example.com', 'cdn.example.com', 5])
      expect(csp([bad]), String(bad)).toContain('x-app.csp.resourceDomains[0]')
    expect(problem(doc({ 'x-app': { id: 'feeds', csp: { scriptDomains: [] } } }))).toContain('x-app.csp.scriptDomains: is not supported')
    expect(problem(doc({ 'x-app': { id: 'feeds', permissions: { camera: { x: 1 } } } }))).toContain('must be {}')
    expect(problem(doc({ 'x-app': { id: 'feeds', permissions: { nfc: {} } } }))).toContain('x-app.permissions.nfc: is not supported')
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
    expect(problem(withSchema({ anyOf: [{ type: 'string' }] }))).toContain('.anyOf: is not supported')
    expect(problem(withSchema({ oneOf: [{ type: 'string' }, { type: 'integer' }] }))).toContain('.oneOf: is not supported')
    expect(problem(withSchema({ $ref: '#/components/schemas/Missing' }))).toContain('refers to `Missing`')
    expect(problem(withSchema({ $ref: 'http://elsewhere/x' }))).toContain('must be "#/components/schemas/<Name>"')
    expect(problem(withSchema({ type: 'array' }))).toContain('needs `items`')
    expect(problem(withSchema({ type: 'object', properties: { a: { type: 'null' } }, required: ['b'] }))).toContain(
      'names `b`, which is not a property'
    )
    expect(problem(withSchema({ type: 'string', enum: [] }))).toContain('non-empty list of strings')
    expect(problem(withSchema({ type: 'integer', minimum: 'one' }))).toContain('.minimum: must be a number')
  })

  test('a name the language gives to every object is never a name of the contract, and nothing is inherited', () => {
    const withProps = (properties: string, extra = ''): string =>
      `{"openrpc":"1.3.2","info":{"title":"N","version":"1.0.0"},"x-app":{"id":"notes"},"methods":[{"name":"a","result":{"name":"r","schema":{"$ref":"#/components/schemas/T"}}}],"components":{"schemas":{"T":{"type":"object","properties":${properties}${extra}}}}}`
    // `JSON.parse` makes `__proto__` an own key: it is refused, not dropped
    expect(problem(withProps('{"__proto__":{"type":"string"}}'))).toContain('`__proto__` cannot be a property name')
    expect(problem(withProps('{"constructor":{"type":"string"}}'))).toContain('cannot be a property name')
    expect(problem(withProps('{"prototype":{"type":"string"}}'))).toContain('cannot be a property name')
    expect(problem(withProps('{"a":{"type":"string"}}', ',"required":["toString"]'))).toContain('names `toString`, which is not a property')
    expect(problem(withProps('{"a":{"$ref":"#/components/schemas/constructor"}}'))).toContain('refers to `constructor`, which components.schemas does not define')
    expect(problem(withProps('{"a":{"$ref":"#/components/schemas/__proto__"}}'))).toContain('refers to `__proto__`')
    expect(problem(doc({ methods: [{ name: 'constructor', result: { name: 'r', schema: { type: 'null' } } }] }))).toContain('cannot be a method name')
    expect(problem(doc({ components: { schemas: JSON.parse('{"__proto__":{"type":"string"}}') } }))).toContain('cannot be a schema name')
  })

  test('words a language reserves are fine as params and properties, and refused only where they would break the generated code', () => {
    const ok = parseDefinition(
      doc({
        methods: [
          {
            name: 'move',
            params: ['type', 'default', 'class', 'new', 'delete', 'end'].map((name) => ({ name, schema: { type: 'string' } })),
            result: { name: 'r', schema: { type: 'null' } }
          }
        ]
      })
    )
    expect(ok.methods[0]?.params.map((p) => p.name)).toEqual(['type', 'default', 'class', 'new', 'delete', 'end'])
    // a method is a Luau function field, a schema a type
    expect(problem(doc({ methods: [{ name: 'end', result: { name: 'r', schema: { type: 'null' } } }] }))).toContain('reserved word in Luau')
    expect(problem(doc({ components: { schemas: { class: { type: 'string' } } } }))).toContain('reserved word')
    expect(parseDefinition(doc({ methods: [{ name: 'delete', result: { name: 'r', schema: { type: 'null' } } }] })).methods[0]?.name).toBe('delete')
  })

  test('two generated types with one name are found when the contract is read, and say which clash', () => {
    const result = { name: 'r', schema: { type: 'null' } }
    for (const clash of ['Host', 'Api', 'RpcError', 'Context', 'Handlers']) {
      expect(problem(doc({ methods: [{ name: 'a', result }], components: { schemas: { [clash]: { type: 'string' } } } })), clash).toContain(`the generated type \`${clash}\``)
    }
    expect(
      problem(doc({ methods: [{ name: 'list', result }], components: { schemas: { ListResult: { type: 'string' } } } }))
    ).toContain('`ListResult` would be both the schema `ListResult` and the result of method `list`')
    expect(problem(doc({ methods: [{ name: 'add_task', result }, { name: 'addTask', result }] }))).toContain(
      '`AddTaskResult` would be both the result of method `add_task` and the result of method `addTask`'
    )
  })

  test('a JSON syntax error says where, and an unsupported combinator says it is not supported', () => {
    expect(problem('{\n  "openrpc": "1.3.2",\n  "info": {\n    "title": "N",,\n  }\n}')).toContain('line 4, column 18')
    expect(problem('{"a": [1, 2')).toContain('line 1, column 12')
    expect(problem('')).toContain('line 1, column 1')
    expect(jsonErrorOffset('{"a": [1, 2, {"b": null}]}')).toBeNull()
    expect(jsonErrorOffset('{"a": tru}')).toBe(6)
    expect(jsonErrorOffset('[1] x')).toBe(4)
  })
})
