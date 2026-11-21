/**
 * The app's RPC definition: an OpenRPC document (https://spec.open-rpc.org/), the standard description format for
 * JSON-RPC 2.0 services, kept as a file in the app's project (`rpc.openrpc.json`). It is the contract between the
 * app's client and its Luau backend, and everything the plugin generates comes from it.
 *
 * What a definition may say (a deliberate subset: the basic version of the contract):
 *
 *   - `methods`: each has a `name` (an identifier: it becomes `rpc.<name>(...)` in the client and a function in the Luau
 *     handlers), `params` (by name only: `paramStructure` is `"by-name"` or absent), a `result`, and optional `errors`
 *     (documentation of the application errors a handler may fail with).
 *   - `components.schemas`: named schemas other schemas refer to with `{ "$ref": "#/components/schemas/Name" }`.
 *   - schemas, a subset of JSON Schema: `type` (`string`, `number`, `integer`, `boolean`, `null`, `array`, `object`, or a
 *     list of one of these and `"null"`), `enum` of strings, `minLength`, `maxLength`, `minimum`, `maximum`, `items`,
 *     `properties` with `required`, and `description`. An object takes no property it does not declare.
 *
 * Anything else is an error that names where it is, so a definition never means something a reader did not expect.
 */

export type Schema = { description?: string } & (
  | { kind: 'string'; enum?: string[]; minLength?: number; maxLength?: number }
  | { kind: 'number'; integer: boolean; minimum?: number; maximum?: number }
  | { kind: 'boolean' }
  | { kind: 'null' }
  | { kind: 'array'; items: Schema }
  | { kind: 'object'; properties: Record<string, Schema>; required: string[] }
  | { kind: 'ref'; name: string }
  | { kind: 'nullable'; inner: Schema }
)

export interface Param {
  name: string
  required: boolean
  schema: Schema
  description?: string
}

export interface Method {
  name: string
  summary?: string
  description?: string
  params: Param[]
  result: Schema
  errors: { code: number; message: string }[]
}

export interface Definition {
  title: string
  version: string
  methods: Method[]
  /** Named schemas, in the order the document lists them. */
  components: Record<string, Schema>
}

export class DefinitionError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'DefinitionError'
  }
}

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/
/** Words neither TypeScript nor Luau lets a name be where the generated code needs one. */
const RESERVED = new Set([
  'and', 'break', 'class', 'const', 'continue', 'default', 'delete', 'do', 'else', 'elseif', 'end', 'enum', 'export',
  'false', 'for', 'function', 'if', 'import', 'in', 'interface', 'let', 'local', 'new', 'nil', 'not', 'null', 'or',
  'repeat', 'return', 'switch', 'then', 'this', 'true', 'type', 'typeof', 'until', 'var', 'void', 'while', 'with'
])

type Json = unknown
const isObject = (v: Json): v is Record<string, Json> => typeof v === 'object' && v !== null && !Array.isArray(v)

function fail(path: string, message: string): never {
  throw new DefinitionError(`${path}: ${message}`)
}

function text(value: Json, path: string): string {
  if (typeof value !== 'string' || value === '') fail(path, 'must be a non-empty string')
  return value
}

function optionalText(value: Json, path: string): string | undefined {
  return value === undefined ? undefined : text(value, path)
}

function identifier(value: Json, path: string, what: string): string {
  const name = text(value, path)
  if (!IDENTIFIER.test(name)) fail(path, `${what} \`${name}\` must be an identifier (letters, digits and _, not starting with a digit)`)
  if (RESERVED.has(name)) fail(path, `\`${name}\` is a reserved word in TypeScript or Luau: pick another ${what}`)
  return name
}

function count(value: Json, path: string): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) fail(path, 'must be a whole number, 0 or more')
  return value
}

function only(object: Record<string, Json>, allowed: string[], path: string): void {
  for (const key of Object.keys(object)) {
    if (!allowed.includes(key)) fail(`${path}.${key}`, `is not supported (this schema may use: ${allowed.join(', ')})`)
  }
}

const REF = /^#\/components\/schemas\/([A-Za-z_][A-Za-z0-9_]*)$/

/** One schema of the supported subset. `refs` collects the names a `$ref` uses, to be checked against the components. */
function schema(value: Json, path: string, refs: { name: string; path: string }[]): Schema {
  if (!isObject(value)) fail(path, 'must be a schema object')
  const description = optionalText(value.description, `${path}.description`)
  const withDescription = <T extends object>(node: T): T & { description?: string } =>
    description === undefined ? node : { ...node, description }

  if (value.$ref !== undefined) {
    only(value, ['$ref', 'description'], path)
    const match = typeof value.$ref === 'string' ? REF.exec(value.$ref) : null
    if (!match) fail(`${path}.$ref`, 'must be "#/components/schemas/<Name>"')
    const name = match[1] as string
    refs.push({ name, path: `${path}.$ref` })
    return withDescription({ kind: 'ref' as const, name })
  }

  let types: string[]
  if (typeof value.type === 'string') types = [value.type]
  else if (Array.isArray(value.type) && value.type.length === 2 && value.type.includes('null') && value.type.every((t) => typeof t === 'string'))
    types = (value.type as string[]).filter((t) => t !== 'null').concat(['null'])
  else fail(`${path}.type`, 'must be a type name, or a list of a type name and "null"')
  const nullable = types.length === 2
  const type = types[0] as string

  const inner = ((): Schema => {
    switch (type) {
      case 'string': {
        only(value, ['type', 'description', 'enum', 'minLength', 'maxLength'], path)
        let options: string[] | undefined
        if (value.enum !== undefined) {
          if (!Array.isArray(value.enum) || value.enum.length === 0 || !value.enum.every((o) => typeof o === 'string'))
            fail(`${path}.enum`, 'must be a non-empty list of strings')
          options = value.enum as string[]
        }
        const node: Extract<Schema, { kind: 'string' }> = { kind: 'string' }
        if (options) node.enum = options
        if (value.minLength !== undefined) node.minLength = count(value.minLength, `${path}.minLength`)
        if (value.maxLength !== undefined) node.maxLength = count(value.maxLength, `${path}.maxLength`)
        return node
      }
      case 'number':
      case 'integer': {
        only(value, ['type', 'description', 'minimum', 'maximum'], path)
        const node: Extract<Schema, { kind: 'number' }> = { kind: 'number', integer: type === 'integer' }
        for (const bound of ['minimum', 'maximum'] as const) {
          const given = value[bound]
          if (given === undefined) continue
          if (typeof given !== 'number' || !Number.isFinite(given)) fail(`${path}.${bound}`, 'must be a number')
          node[bound] = given
        }
        return node
      }
      case 'boolean':
        only(value, ['type', 'description'], path)
        return { kind: 'boolean' }
      case 'null':
        only(value, ['type', 'description'], path)
        return { kind: 'null' }
      case 'array':
        only(value, ['type', 'description', 'items'], path)
        if (value.items === undefined) fail(`${path}.items`, 'an array needs `items`')
        return { kind: 'array', items: schema(value.items, `${path}.items`, refs) }
      case 'object': {
        only(value, ['type', 'description', 'properties', 'required'], path)
        const properties: Record<string, Schema> = {}
        if (value.properties !== undefined) {
          if (!isObject(value.properties)) fail(`${path}.properties`, 'must be an object of schemas')
          for (const [name, property] of Object.entries(value.properties)) {
            identifier(name, `${path}.properties.${name}`, 'property name')
            properties[name] = schema(property, `${path}.properties.${name}`, refs)
          }
        }
        const required: string[] = []
        if (value.required !== undefined) {
          if (!Array.isArray(value.required)) fail(`${path}.required`, 'must be a list of property names')
          for (const name of value.required) {
            if (typeof name !== 'string' || !(name in properties)) fail(`${path}.required`, `names \`${String(name)}\`, which is not a property`)
            if (!required.includes(name)) required.push(name)
          }
        }
        return { kind: 'object', properties, required }
      }
      default:
        return fail(`${path}.type`, `\`${type}\` is not supported (string, number, integer, boolean, null, array, object)`)
    }
  })()
  const node: Schema = nullable ? { kind: 'nullable', inner } : inner
  return withDescription(node)
}

/** Parse and validate an OpenRPC document. Throws a [`DefinitionError`] naming the path of the first problem. */
export function parseDefinition(source: string, file = 'rpc.openrpc.json'): Definition {
  let document: Json
  try {
    document = JSON.parse(source)
  } catch (e) {
    throw new DefinitionError(`${file}: not JSON (${(e as Error).message})`)
  }
  try {
    return definition(document)
  } catch (e) {
    if (e instanceof DefinitionError) throw new DefinitionError(`${file}: ${e.message}`)
    throw e
  }
}

function definition(document: Json): Definition {
  if (!isObject(document)) fail('$', 'must be an object')
  const openrpc = text(document.openrpc, 'openrpc')
  if (!/^1\.\d+(\.\d+)?$/.test(openrpc)) fail('openrpc', `version ${openrpc} is not supported (1.x)`)
  if (!isObject(document.info)) fail('info', 'must be an object with a title and a version')
  const title = text(document.info.title, 'info.title')
  const version = text(document.info.version, 'info.version')

  const refs: { name: string; path: string }[] = []
  const components: Record<string, Schema> = {}
  if (document.components !== undefined) {
    if (!isObject(document.components)) fail('components', 'must be an object')
    only(document.components, ['schemas'], 'components')
    const schemas = document.components.schemas ?? {}
    if (!isObject(schemas)) fail('components.schemas', 'must be an object of schemas')
    for (const [name, value] of Object.entries(schemas)) {
      identifier(name, `components.schemas.${name}`, 'schema name')
      components[name] = schema(value, `components.schemas.${name}`, refs)
    }
  }

  if (!Array.isArray(document.methods)) fail('methods', 'must be a list of methods')
  const methods: Method[] = []
  document.methods.forEach((raw, index) => {
    const path = `methods[${index}]`
    if (!isObject(raw)) fail(path, 'must be an object')
    only(raw, ['name', 'summary', 'description', 'paramStructure', 'params', 'result', 'errors', 'tags'], path)
    const name = identifier(raw.name, `${path}.name`, 'method name')
    if (methods.some((m) => m.name === name)) fail(`${path}.name`, `method \`${name}\` is defined twice`)
    if (raw.paramStructure !== undefined && raw.paramStructure !== 'by-name')
      fail(`${path}.paramStructure`, 'must be "by-name": a client calls `rpc.method({ param })`')
    const params: Param[] = []
    if (raw.params !== undefined) {
      if (!Array.isArray(raw.params)) fail(`${path}.params`, 'must be a list')
      raw.params.forEach((p, i) => {
        const where = `${path}.params[${i}]`
        if (!isObject(p)) fail(where, 'must be an object')
        only(p, ['name', 'description', 'required', 'schema', 'summary'], where)
        const paramName = identifier(p.name, `${where}.name`, 'param name')
        if (params.some((q) => q.name === paramName)) fail(`${where}.name`, `param \`${paramName}\` is given twice`)
        if (p.required !== undefined && typeof p.required !== 'boolean') fail(`${where}.required`, 'must be true or false')
        const description = optionalText(p.description, `${where}.description`)
        params.push({
          name: paramName,
          required: p.required === true,
          schema: schema(p.schema, `${where}.schema`, refs),
          ...(description === undefined ? {} : { description })
        })
      })
    }
    if (!isObject(raw.result)) fail(`${path}.result`, 'a method needs a `result` with a name and a schema')
    only(raw.result, ['name', 'description', 'schema', 'summary'], `${path}.result`)
    text(raw.result.name, `${path}.result.name`)
    const result = schema(raw.result.schema, `${path}.result.schema`, refs)
    const errors: { code: number; message: string }[] = []
    if (raw.errors !== undefined) {
      if (!Array.isArray(raw.errors)) fail(`${path}.errors`, 'must be a list')
      raw.errors.forEach((e, i) => {
        const where = `${path}.errors[${i}]`
        if (!isObject(e) || typeof e.code !== 'number' || !Number.isInteger(e.code)) fail(where, 'must be { code: whole number, message }')
        errors.push({ code: e.code, message: text(e.message, `${where}.message`) })
      })
    }
    const summary = optionalText(raw.summary, `${path}.summary`)
    const description = optionalText(raw.description, `${path}.description`)
    methods.push({
      name,
      params,
      result,
      errors,
      ...(summary === undefined ? {} : { summary }),
      ...(description === undefined ? {} : { description })
    })
  })

  for (const { name, path } of refs) if (!(name in components)) fail(path, `refers to \`${name}\`, which components.schemas does not define`)
  return { title, version, methods, components }
}
