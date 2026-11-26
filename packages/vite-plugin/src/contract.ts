/**
 * The app's contract: an OpenRPC 1.4 document (`openrpc.json`) that types everything the app's backend serves and sends.
 *
 *   - A method with a `result` is a request the app serves: its page, other apps and agents send it.
 *   - A method without a `result` is a notification (OpenRPC: "If undefined, the method MUST only be used as a
 *     notification"). Only the app itself sends its notifications; its page, other apps and agents listen.
 *   - `foo` is public: other apps and agents may use it, and a public request is an agent tool. `_foo` is private: meant
 *     for the app's own page. It is a convention, as in Python, not a wall.
 *   - `x-readOnly: true` on a request: it changes nothing, so the host runs it read-only when an agent calls it.
 *
 * What the app is (its id, name, version, icon) is its `package.json`'s (`app.ts`), not the contract's.
 *
 * Schemas are a subset of JSON Schema: `type` (`string`, `number`, `integer`, `boolean`, `null`, `array`, `object`, or a
 * list of one of these and `"null"`), `enum` of strings, `minLength`, `maxLength`, `minimum`, `maximum`, `items`,
 * `properties` with `required`, `description`, and `$ref` to `#/components/schemas/<Name>`. Anything else is an error
 * that names where it is.
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

interface MethodBase {
  name: string
  /** `_foo`: meant for the app's own page. */
  private: boolean
  summary?: string
  description?: string
  params: Param[]
}

export interface Request extends MethodBase {
  kind: 'request'
  result: Schema
  readOnly: boolean
}

export interface Notification extends MethodBase {
  kind: 'notification'
}

export type Method = Request | Notification

export interface Contract {
  requests: Request[]
  notifications: Notification[]
  /** Named schemas, in the order the document lists them. */
  components: Record<string, Schema>
}

export class ContractError extends Error {
  override name = 'ContractError'
}

type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
type JsonObject = { [key: string]: Json }

const fail = (path: string, message: string): never => {
  throw new ContractError(`${path} ${message}`)
}
const isObject = (v: Json | undefined): v is JsonObject => typeof v === 'object' && v !== null && !Array.isArray(v)
const table = <T>(): Record<string, T> => Object.create(null) as Record<string, T>
const has = (o: object, key: string): boolean => Object.prototype.hasOwnProperty.call(o, key)

/** Keys an object may have; `x-` extensions are allowed where OpenRPC allows them and ignored unless named. */
function only(value: JsonObject, keys: string[], path: string, extensions = false): void {
  for (const key of Object.keys(value)) {
    if (keys.includes(key) || (extensions && key.startsWith('x-'))) continue
    fail(`${path}.${key}`, `is not supported (one of: ${keys.join(', ')})`)
  }
}

function text(value: Json | undefined, path: string): string {
  if (typeof value !== 'string' || value.trim() === '') fail(path, 'must be a non-empty string')
  return value as string
}

function optionalText(value: Json | undefined, path: string): string | undefined {
  return value === undefined ? undefined : text(value, path)
}

function count(value: Json | undefined, path: string): number | undefined {
  if (value === undefined) return undefined
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) fail(path, 'must be a whole number, 0 or more')
  return value as number
}

function bound(value: Json | undefined, path: string): number | undefined {
  if (value === undefined) return undefined
  if (typeof value !== 'number' || !Number.isFinite(value)) fail(path, 'must be a number')
  return value as number
}

/** `foo` or `_foo`: a letter, then letters, digits and `_`, at most 48 characters. */
export const METHOD_NAME = /^_?[a-z][A-Za-z0-9_]{0,46}$/
/** A component is a type name in both languages. */
const COMPONENT_NAME = /^[A-Z][A-Za-z0-9]{0,63}$/
const PARAM_NAME = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/
const COMPONENT_REF = /^#\/components\/schemas\/(.+)$/

const SCHEMA_KEYS = ['type', 'description', 'enum', 'minLength', 'maxLength', 'minimum', 'maximum', 'items', 'properties', 'required', '$ref', 'title']

function schema(raw: Json | undefined, path: string, refs: { name: string; path: string }[]): Schema {
  if (!isObject(raw)) return fail(path, 'must be a schema (an object)')
  only(raw, SCHEMA_KEYS, path)
  const description = optionalText(raw.description, `${path}.description`)
  const withDescription = (s: Schema): Schema => (description === undefined ? s : { ...s, description })
  if (raw.$ref !== undefined) {
    only(raw, ['$ref', 'description', 'title'], path)
    const match = COMPONENT_REF.exec(text(raw.$ref, `${path}.$ref`))
    if (!match) fail(`${path}.$ref`, 'must be #/components/schemas/<Name>')
    const name = (match as RegExpExecArray)[1] as string
    refs.push({ name, path: `${path}.$ref` })
    return withDescription({ kind: 'ref', name })
  }
  let type = raw.type
  let nullable = false
  if (Array.isArray(type)) {
    const others = type.filter((t) => t !== 'null')
    if (type.length !== 2 || others.length !== 1) fail(`${path}.type`, 'as a list must be one type and "null"')
    nullable = true
    type = others[0] as Json
  }
  const wrap = (s: Schema): Schema => withDescription(nullable ? { kind: 'nullable', inner: s } : s)
  switch (type) {
    case 'string': {
      let options: string[] | undefined
      if (raw.enum !== undefined) {
        if (!Array.isArray(raw.enum) || raw.enum.length === 0 || raw.enum.some((o) => typeof o !== 'string'))
          fail(`${path}.enum`, 'must be a non-empty list of strings')
        options = raw.enum as string[]
      }
      const minLength = count(raw.minLength, `${path}.minLength`)
      const maxLength = count(raw.maxLength, `${path}.maxLength`)
      return wrap({
        kind: 'string',
        ...(options ? { enum: options } : {}),
        ...(minLength === undefined ? {} : { minLength }),
        ...(maxLength === undefined ? {} : { maxLength })
      })
    }
    case 'number':
    case 'integer': {
      const minimum = bound(raw.minimum, `${path}.minimum`)
      const maximum = bound(raw.maximum, `${path}.maximum`)
      return wrap({
        kind: 'number',
        integer: type === 'integer',
        ...(minimum === undefined ? {} : { minimum }),
        ...(maximum === undefined ? {} : { maximum })
      })
    }
    case 'boolean':
      return wrap({ kind: 'boolean' })
    case 'null':
      return withDescription({ kind: 'null' })
    case 'array':
      return wrap({ kind: 'array', items: schema(raw.items, `${path}.items`, refs) })
    case 'object': {
      const properties = table<Schema>()
      if (raw.properties !== undefined) {
        if (!isObject(raw.properties)) fail(`${path}.properties`, 'must be an object of schemas')
        for (const [name, property] of Object.entries(raw.properties as JsonObject)) {
          if (!PARAM_NAME.test(name)) fail(`${path}.properties.${name}`, 'must be named with letters, digits and _')
          properties[name] = schema(property, `${path}.properties.${name}`, refs)
        }
      }
      const required: string[] = []
      if (raw.required !== undefined) {
        if (!Array.isArray(raw.required)) fail(`${path}.required`, 'must be a list of property names')
        for (const name of raw.required as Json[]) {
          if (typeof name !== 'string' || !has(properties, name)) fail(`${path}.required`, `names \`${String(name)}\`, which is not a property`)
          required.push(name as string)
        }
      }
      return wrap({ kind: 'object', properties, required })
    }
    default:
      return fail(`${path}.type`, 'must be string, number, integer, boolean, null, array or object')
  }
}

function params(raw: Json | undefined, path: string, refs: { name: string; path: string }[]): Param[] {
  if (!Array.isArray(raw)) return fail(path, 'must be a list (OpenRPC requires `params`, `[]` for none)')
  const list: Param[] = []
  raw.forEach((p, i) => {
    const where = `${path}[${i}]`
    if (!isObject(p)) fail(where, 'must be a content descriptor (an object)')
    const param = p as JsonObject
    only(param, ['name', 'summary', 'description', 'required', 'schema'], where, true)
    const name = text(param.name, `${where}.name`)
    if (!PARAM_NAME.test(name)) fail(`${where}.name`, 'must be letters, digits and _')
    if (list.some((q) => q.name === name)) fail(`${where}.name`, `\`${name}\` is given twice`)
    if (param.required !== undefined && typeof param.required !== 'boolean') fail(`${where}.required`, 'must be true or false')
    const description = optionalText(param.description, `${where}.description`)
    list.push({
      name,
      required: param.required === true,
      schema: schema(param.schema, `${where}.schema`, refs),
      ...(description === undefined ? {} : { description })
    })
  })
  return list
}

/** `add_task` and `addTask` (and `_addTask`) are all `AddTask`: the name of a method's generated types. */
export function pascal(name: string): string {
  return name
    .split('_')
    .filter((w) => w !== '')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join('')
}

/** `addSite` is the agent tool `add_site` (a tool name is `[a-z][a-z0-9_]*`). */
export function toolName(method: string): string {
  return method.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`)
}

/** Parse and check an OpenRPC document. Throws a [`ContractError`] naming the path of the first problem. */
export function parseContract(source: string, file = 'openrpc.json'): Contract {
  let document: Json
  try {
    document = JSON.parse(source) as Json
  } catch (e) {
    throw new ContractError(`${file}: not JSON (${(e as Error).message})`)
  }
  try {
    return contract(document)
  } catch (e) {
    if (e instanceof ContractError) throw new ContractError(`${file}: ${e.message}`)
    throw e
  }
}

function contract(document: Json): Contract {
  if (!isObject(document)) return fail('$', 'must be an object')
  only(document, ['openrpc', 'info', 'methods', 'components', '$schema'], '$', true)
  const version = text(document.openrpc, 'openrpc')
  if (!/^1\.4\.\d+$/.test(version)) fail('openrpc', `must be 1.4.x (it is ${version})`)
  if (!isObject(document.info)) fail('info', 'must be an object with a title and a version (OpenRPC requires it)')

  const refs: { name: string; path: string }[] = []
  const components = table<Schema>()
  if (document.components !== undefined) {
    if (!isObject(document.components)) fail('components', 'must be an object')
    const c = document.components as JsonObject
    only(c, ['schemas'], 'components', true)
    const schemas = c.schemas ?? {}
    if (!isObject(schemas)) fail('components.schemas', 'must be an object of schemas')
    for (const [name, raw] of Object.entries(schemas as JsonObject)) {
      const path = `components.schemas.${name}`
      if (!COMPONENT_NAME.test(name)) fail(path, 'must be a type name: a capital letter, then letters and digits')
      components[name] = schema(raw, path, refs)
    }
  }

  if (!Array.isArray(document.methods)) fail('methods', 'must be a list of methods')
  const requests: Request[] = []
  const notifications: Notification[] = []
  const seen = new Set<string>()
  ;(document.methods as Json[]).forEach((raw, index) => {
    const path = `methods[${index}]`
    if (!isObject(raw)) fail(path, 'must be an object')
    const m = raw as JsonObject
    only(m, ['name', 'summary', 'description', 'paramStructure', 'params', 'result', 'errors', 'tags', 'deprecated', 'examples', 'externalDocs'], path, true)
    for (const key of Object.keys(m)) if (key.startsWith('x-') && key !== 'x-readOnly') fail(`${path}.${key}`, 'is not an extension the plugin knows (x-readOnly)')
    const name = text(m.name, `${path}.name`)
    if (!METHOD_NAME.test(name)) fail(`${path}.name`, 'must be `foo` (public) or `_foo` (private): a letter, then letters, digits and _, at most 48')
    if (seen.has(name)) fail(`${path}.name`, `\`${name}\` is defined twice`)
    const twin = name.startsWith('_') ? name.slice(1) : `_${name}`
    if (seen.has(twin)) fail(`${path}.name`, `\`${name}\` and \`${twin}\` are both defined: one name is public or private, not both`)
    seen.add(name)
    if (m.paramStructure !== undefined && m.paramStructure !== 'by-name') fail(`${path}.paramStructure`, 'must be "by-name": a call names its params')
    const base = {
      name,
      private: name.startsWith('_'),
      params: params(m.params, `${path}.params`, refs),
      ...(m.summary === undefined ? {} : { summary: text(m.summary, `${path}.summary`) }),
      ...(m.description === undefined ? {} : { description: text(m.description, `${path}.description`) })
    }
    if (m.result === undefined) {
      if (m['x-readOnly'] !== undefined) fail(`${path}.x-readOnly`, 'is for a request (a method with a result), not a notification')
      notifications.push({ kind: 'notification', ...base })
      return
    }
    if (!isObject(m.result)) fail(`${path}.result`, 'must be a content descriptor with a name and a schema')
    const r = m.result as JsonObject
    only(r, ['name', 'summary', 'description', 'schema', 'required'], `${path}.result`, true)
    text(r.name, `${path}.result.name`)
    if (m['x-readOnly'] !== undefined && typeof m['x-readOnly'] !== 'boolean') fail(`${path}.x-readOnly`, 'must be true or false')
    requests.push({ kind: 'request', ...base, result: schema(r.schema, `${path}.result.schema`, refs), readOnly: m['x-readOnly'] === true })
  })

  for (const { name, path } of refs) if (!has(components, name)) fail(path, `refers to \`${name}\`, which components.schemas does not define`)
  checkGeneratedNames(components, [...requests, ...notifications])
  checkToolNames(requests)
  return { requests, notifications, components }
}

/** Every generated type name (`<Method>Params`, `<Request>Result`) is one name, and no component's. */
function checkGeneratedNames(components: Record<string, Schema>, methods: Method[]): void {
  const owner = new Map<string, string>(Object.keys(components).map((c) => [c, `components.schemas.${c}`]))
  for (const method of methods) {
    const names = [`${pascal(method.name)}Params`, ...(method.kind === 'request' ? [`${pascal(method.name)}Result`] : [])]
    for (const type of names) {
      const other = owner.get(type)
      if (other !== undefined) fail(`method \`${method.name}\``, `generates the type ${type}, which ${other} names too`)
      owner.set(type, `method \`${method.name}\``)
    }
  }
}

/** Two public requests may not become the same agent tool (`addSite` and `add_site`). */
function checkToolNames(requests: Request[]): void {
  const tools = new Map<string, string>()
  for (const r of requests.filter((r) => !r.private)) {
    const tool = toolName(r.name)
    if (tool.length > 48) fail(`method \`${r.name}\``, `is the agent tool \`${tool}\`, longer than a tool name may be (48)`)
    const other = tools.get(tool)
    if (other !== undefined) fail(`method \`${r.name}\``, `and \`${other}\` are both the agent tool \`${tool}\``)
    tools.set(tool, r.name)
  }
}

/** A method's params as one object schema. */
export function paramsObject(method: Method): Schema & { kind: 'object' } {
  const properties = table<Schema>()
  for (const p of method.params) properties[p.name] = p.schema
  return { kind: 'object', properties, required: method.params.filter((p) => p.required).map((p) => p.name) }
}

/**
 * Whether a value of this schema may be absent. In this contract a null and an absent key are one value (a Luau nil is an
 * absent key), so a property or param whose schema allows null is never required, in either language.
 */
export function allowsNull(schema: Schema, components: Record<string, Schema>, seen = new Set<string>()): boolean {
  if (schema.kind === 'nullable' || schema.kind === 'null') return true
  if (schema.kind !== 'ref' || seen.has(schema.name)) return false
  seen.add(schema.name)
  const target = components[schema.name]
  return target !== undefined && allowsNull(target, components, seen)
}
