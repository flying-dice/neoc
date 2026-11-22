/**
 * The app's RPC definition: an OpenRPC document (https://spec.open-rpc.org/), the standard description format for
 * JSON-RPC 2.0 services, kept as a file in the app's project (`rpc.openrpc.json`). It is the contract between the
 * app's client and its Luau backend, and everything the plugin generates comes from it.
 *
 * What a definition may say (a deliberate subset: the basic version of the contract):
 *
 *   - `info.title` and `info.version`: the app's name and version.
 *   - `x-app`: the app's metadata (`id`, `icon`, `tint`, `description`, `window`, `requires`, `migrations`), so the contract is
 *     the only descriptor an app needs.
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

/** What the shell needs to know about the app before any code runs: the `x-app` extension of the contract. */
export interface AppMeta {
  /** 3 to 40 characters: lowercase letters, digits, dashes; namespaces the app's data and tools. */
  id: string
  icon?: string
  tint?: string
  description?: string
  /** The default window and its minimum size. */
  window?: { w: number; h: number; minW: number; minH: number }
  requires?: { host?: string }
  migrations?: { version: number; name: string; file: string }[]
  /**
   * What the app's frame may load, in the shape of MCP Apps' `_meta.ui.csp`: the host builds the frame's content security
   * policy from it (nothing external when it is absent). `resourceDomains` are origins images, media, fonts and styles may
   * come from; `connectDomains`, `frameDomains` and `baseUriDomains` are parsed but not applied (an app's calls go through
   * the host). The app's code still comes only from the host.
   */
  csp?: Csp
  /** The browser capabilities the frame asks for, in the shape of MCP Apps' `_meta.ui.permissions`; passed to the iframe's `allow`. */
  permissions?: Partial<Record<(typeof PERMISSIONS)[number], Record<string, never>>>
}

export interface Csp {
  connectDomains?: string[]
  resourceDomains?: string[]
  frameDomains?: string[]
  baseUriDomains?: string[]
}

/** The permissions MCP Apps names. */
export const PERMISSIONS = ['camera', 'microphone', 'geolocation', 'clipboardWrite'] as const

/** An origin an app declares: `https://host[:port]`, or `https://*.domain.tld` for the subdomains of one domain. Never a bare scheme or `*`. */
const ORIGIN = /^https?:\/\/(\*\.)?(([a-z0-9]([a-z0-9-]*[a-z0-9])?)\.)+[a-z]{2,}(:\d{1,5})?$|^https?:\/\/(localhost|\d{1,3}(\.\d{1,3}){3})(:\d{1,5})?$/i

export interface Definition {
  /** The app's name (`info.title`). */
  title: string
  /** The app's version (`info.version`, `major.minor.patch`). */
  version: string
  app: AppMeta
  methods: Method[]
  /** Named schemas, in the order the document lists them. */
  components: Record<string, Schema>
}

/** The names the generated files claim for themselves: a component or a method's types cannot reuse them. */
const GENERATED_TS = ['Host', 'Api', 'RpcError']
const GENERATED_LUAU = ['Context', 'Handlers']

/** PascalCase of a method name: `add_task` and `addTask` both become `AddTask`. */
const pascalName = (name: string): string =>
  name
    .split('_')
    .filter((w) => w !== '')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join('') || name

/**
 * The generated files declare one type per component and, per method, `<Method>Params` and `<Method>Result`, next to a few
 * fixed ones. Two of them with one name would fail only inside `.app/`, far from the contract: say which clash here.
 */
function checkGeneratedNames(components: Record<string, Schema>, methods: Method[]): void {
  const owners = new Map<string, string>()
  const claim = (name: string, owner: string, path: string): void => {
    const existing = owners.get(name)
    if (existing !== undefined) {
      fail(path, `the generated type \`${name}\` would be both ${existing} and ${owner}: rename one of them`)
    }
    owners.set(name, owner)
  }
  for (const name of [...GENERATED_TS, ...GENERATED_LUAU]) owners.set(name, 'a type the generated files declare themselves')
  for (const name of Object.keys(components)) claim(name, `the schema \`${name}\``, `components.schemas.${name}`)
  methods.forEach((method, index) => {
    const base = pascalName(method.name)
    const where = `methods[${index}].name`
    if (method.params.length > 0) claim(`${base}Params`, `the params of method \`${method.name}\``, where)
    claim(`${base}Result`, `the result of method \`${method.name}\``, where)
  })
}

export class DefinitionError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'DefinitionError'
  }
}

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/

/** Names that mean something to a JavaScript object, whatever the language: never a name of the contract. */
const FORBIDDEN = new Set(['__proto__', 'constructor', 'prototype'])

/** Luau's reserved words: not a method name (a handler is `name = function`) and not the name of a schema (a Luau type). */
export const LUAU_KEYWORDS = new Set([
  'and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for', 'function', 'if', 'in', 'local', 'nil', 'not', 'or',
  'repeat', 'return', 'then', 'true', 'until', 'while'
])

/** Words a TypeScript type cannot be named. */
const TS_TYPE_RESERVED = new Set([
  'any', 'boolean', 'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger', 'default', 'delete', 'do', 'else',
  'enum', 'export', 'extends', 'false', 'finally', 'for', 'function', 'if', 'import', 'in', 'instanceof', 'interface', 'let',
  'never', 'new', 'null', 'number', 'object', 'package', 'private', 'protected', 'public', 'return', 'static', 'string',
  'super', 'switch', 'symbol', 'this', 'throw', 'true', 'try', 'typeof', 'undefined', 'unknown', 'var', 'void', 'while', 'with', 'yield'
])

/** What a name is, for the words it may not be. A property or a param can be any identifier: the generators quote it where a language needs. */
type NameKind = 'property' | 'method' | 'schema'

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

function identifier(value: Json, path: string, what: string, kind: NameKind): string {
  const name = text(value, path)
  if (!IDENTIFIER.test(name)) fail(path, `${what} \`${name}\` must be an identifier (letters, digits and _, not starting with a digit)`)
  if (FORBIDDEN.has(name)) fail(path, `\`${name}\` cannot be a ${what}: it means something to a JavaScript object, so pick another`)
  if (kind === 'method' && LUAU_KEYWORDS.has(name)) fail(path, `\`${name}\` is a reserved word in Luau, where a method is a function field: pick another ${what}`)
  if (kind === 'schema' && (LUAU_KEYWORDS.has(name) || TS_TYPE_RESERVED.has(name))) {
    fail(path, `\`${name}\` is a reserved word in TypeScript or Luau, where a schema is a type: pick another ${what}`)
  }
  return name
}

/** An own key of a parsed object (never one inherited from `Object.prototype`). */
const has = (object: object, key: string): boolean => Object.hasOwn(object, key)

/** A map with no prototype, so no name is ever inherited and `__proto__` is a key like any other. */
const table = <T>(): Record<string, T> => Object.create(null) as Record<string, T>

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

  for (const key of ['oneOf', 'anyOf', 'allOf', 'not', 'if', 'const', 'format', 'pattern', 'patternProperties', 'definitions', '$defs']) {
    if (has(value, key)) {
      fail(`${path}.${key}`, `is not supported: a schema here is a type, \`enum\` of strings, or a list of a type and "null" (see apps/README.md, "The contract")`)
    }
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
        only(value, ['type', 'description', 'properties', 'required', 'additionalProperties'], path)
        if (value.additionalProperties !== undefined && value.additionalProperties !== false) {
          fail(`${path}.additionalProperties`, 'must be false or left out: an object takes no property it does not declare')
        }
        const properties = table<Schema>()
        if (value.properties !== undefined) {
          if (!isObject(value.properties)) fail(`${path}.properties`, 'must be an object of schemas')
          for (const name of Object.keys(value.properties)) {
            identifier(name, `${path}.properties.${name}`, 'property name', 'property')
            properties[name] = schema(value.properties[name], `${path}.properties.${name}`, refs)
          }
        }
        const required: string[] = []
        if (value.required !== undefined) {
          if (!Array.isArray(value.required)) fail(`${path}.required`, 'must be a list of property names')
          for (const name of value.required) {
            if (typeof name !== 'string' || !has(properties, name)) fail(`${path}.required`, `names \`${String(name)}\`, which is not a property`)
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

const lineOf = (source: string, offset: number): number => source.slice(0, offset).split('\n').length
const columnOf = (source: string, offset: number): number => offset - (source.lastIndexOf('\n', offset - 1) + 1) + 1

/** Where a JSON text first stops being JSON (the offset of the character that cannot be there, or of the end), else `null`. */
export function jsonErrorOffset(source: string): number | null {
  let i = 0
  const space = (): void => {
    while (i < source.length && ' \t\n\r'.includes(source[i] as string)) i++
  }
  const literal = (word: string): boolean => {
    if (!source.startsWith(word, i)) return false
    i += word.length
    return true
  }
  const string = (): boolean => {
    if (source[i] !== '"') return false
    i++
    while (i < source.length) {
      const c = source[i] as string
      if (c === '"') {
        i++
        return true
      }
      if (c < ' ') return false
      if (c === '\\') {
        const next = source[i + 1]
        if (next === 'u') {
          if (!/^[0-9a-fA-F]{4}$/.test(source.slice(i + 2, i + 6))) return false
          i += 6
          continue
        }
        if (next === undefined || !'"\\/bfnrt'.includes(next)) return false
        i += 2
        continue
      }
      i++
    }
    return false
  }
  const value = (): boolean => {
    space()
    const c = source[i]
    if (c === '{') {
      i++
      space()
      if (source[i] === '}') return ++i > 0
      for (;;) {
        space()
        if (!string()) return false
        space()
        if (source[i] !== ':') return false
        i++
        if (!value()) return false
        space()
        if (source[i] === ',') {
          i++
          continue
        }
        if (source[i] === '}') return ++i > 0
        return false
      }
    }
    if (c === '[') {
      i++
      space()
      if (source[i] === ']') return ++i > 0
      for (;;) {
        if (!value()) return false
        space()
        if (source[i] === ',') {
          i++
          continue
        }
        if (source[i] === ']') return ++i > 0
        return false
      }
    }
    if (c === '"') return string()
    if (literal('true') || literal('false') || literal('null')) return true
    const number = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?/.exec(source.slice(i))
    if (number) {
      i += number[0].length
      return true
    }
    return false
  }
  if (!value()) return i
  space()
  return i < source.length ? i : null
}

/** Parse and validate an OpenRPC document. Throws a [`DefinitionError`] naming the path of the first problem. */
export function parseDefinition(source: string, file = 'openrpc.json'): Definition {
  let document: Json
  try {
    document = JSON.parse(source)
  } catch (e) {
    const at = jsonErrorOffset(source)
    const where = at === null ? '' : ` at line ${lineOf(source, at)}, column ${columnOf(source, at)}`
    throw new DefinitionError(`${file}: not JSON${where} (${(e as Error).message})`)
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
  if (!/^\d+\.\d+\.\d+$/.test(version)) fail('info.version', 'must be major.minor.patch')
  if (title.length > 60) fail('info.title', 'must be at most 60 characters: it is the name of the app')
  const app = appMeta(document['x-app'])

  const refs: { name: string; path: string }[] = []
  const components = table<Schema>()
  if (document.components !== undefined) {
    if (!isObject(document.components)) fail('components', 'must be an object')
    only(document.components, ['schemas'], 'components')
    const schemas = document.components.schemas ?? {}
    if (!isObject(schemas)) fail('components.schemas', 'must be an object of schemas')
    for (const name of Object.keys(schemas)) {
      identifier(name, `components.schemas.${name}`, 'schema name', 'schema')
      components[name] = schema(schemas[name], `components.schemas.${name}`, refs)
    }
  }

  if (!Array.isArray(document.methods)) fail('methods', 'must be a list of methods')
  const methods: Method[] = []
  document.methods.forEach((raw, index) => {
    const path = `methods[${index}]`
    if (!isObject(raw)) fail(path, 'must be an object')
    only(raw, ['name', 'summary', 'description', 'paramStructure', 'params', 'result', 'errors', 'tags'], path)
    const name = identifier(raw.name, `${path}.name`, 'method name', 'method')
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
        const paramName = identifier(p.name, `${where}.name`, 'param name', 'property')
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

  for (const { name, path } of refs) if (!has(components, name)) fail(path, `refers to \`${name}\`, which components.schemas does not define`)
  checkGeneratedNames(components, methods)
  return { title, version, app, methods, components }
}

/** The `x-app` extension: what the shell needs before the app runs. */
function appMeta(value: Json): AppMeta {
  const path = 'x-app'
  if (!isObject(value)) fail(path, 'is required: the app\'s metadata ({ "id": ..., "icon": ... })')
  only(value, ['id', 'icon', 'tint', 'description', 'window', 'requires', 'migrations', 'csp', 'permissions'], path)
  const id = text(value.id, `${path}.id`)
  if (!/^[a-z0-9][a-z0-9-]{2,39}$/.test(id)) fail(`${path}.id`, 'must be 3 to 40 characters: lowercase letters, digits and dashes')
  const meta: AppMeta = { id }
  const icon = optionalText(value.icon, `${path}.icon`)
  if (icon !== undefined) meta.icon = icon
  const tint = optionalText(value.tint, `${path}.tint`)
  if (tint !== undefined) meta.tint = tint
  const description = optionalText(value.description, `${path}.description`)
  if (description !== undefined) meta.description = description
  if (value.window !== undefined) {
    const window = value.window
    if (!isObject(window)) fail(`${path}.window`, 'must be { w, h, minW, minH }')
    only(window, ['w', 'h', 'minW', 'minH'], `${path}.window`)
    const size = (key: 'w' | 'h' | 'minW' | 'minH'): number => {
      const n = window[key]
      if (typeof n !== 'number' || !Number.isInteger(n) || n <= 0) fail(`${path}.window.${key}`, 'must be a whole number of points')
      return n
    }
    meta.window = { w: size('w'), h: size('h'), minW: size('minW'), minH: size('minH') }
  }
  if (value.requires !== undefined) {
    if (!isObject(value.requires)) fail(`${path}.requires`, 'must be { host }')
    only(value.requires, ['host'], `${path}.requires`)
    const host = optionalText(value.requires.host, `${path}.requires.host`)
    meta.requires = host === undefined ? {} : { host }
  }
  if (value.migrations !== undefined) {
    if (!Array.isArray(value.migrations)) fail(`${path}.migrations`, 'must be a list of { version, name, file }')
    meta.migrations = value.migrations.map((m, i) => {
      const where = `${path}.migrations[${i}]`
      if (!isObject(m)) fail(where, 'must be { version, name, file }')
      only(m, ['version', 'name', 'file'], where)
      return { version: count(m.version, `${where}.version`), name: text(m.name, `${where}.name`), file: text(m.file, `${where}.file`) }
    })
  }
  if (value.csp !== undefined) {
    if (!isObject(value.csp)) fail(`${path}.csp`, 'must be { resourceDomains, connectDomains, frameDomains, baseUriDomains }')
    const keys = ['connectDomains', 'resourceDomains', 'frameDomains', 'baseUriDomains'] as const
    only(value.csp, [...keys], `${path}.csp`)
    const csp: Csp = {}
    for (const key of keys) {
      const list = value.csp[key]
      if (list === undefined) continue
      if (!Array.isArray(list)) fail(`${path}.csp.${key}`, 'must be a list of origins')
      csp[key] = list.map((origin, i) => {
        if (typeof origin !== 'string' || !ORIGIN.test(origin))
          fail(`${path}.csp.${key}[${i}]`, 'must be an origin such as https://cdn.example.com or https://*.example.com (no bare scheme, no `*`)')
        return origin
      })
    }
    meta.csp = csp
  }
  if (value.permissions !== undefined) {
    if (!isObject(value.permissions)) fail(`${path}.permissions`, 'must be an object such as { "camera": {} }')
    only(value.permissions, [...PERMISSIONS], `${path}.permissions`)
    const permissions: NonNullable<AppMeta['permissions']> = {}
    for (const name of PERMISSIONS) {
      const given = value.permissions[name]
      if (given === undefined) continue
      if (!isObject(given) || Object.keys(given).length > 0) fail(`${path}.permissions.${name}`, 'must be {}')
      permissions[name] = {}
    }
    meta.permissions = permissions
  }
  return meta
}
