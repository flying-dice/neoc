/**
 * Types for the backend's ambient API: the global `RPC` the host's VM gives every app (`RPC.send_request`,
 * `RPC.on_request`, `RPC.send_notification`, `RPC.on_notification`), typed for this app and the apps it depends on. The
 * output is one luau-lsp definitions file, `types/vos.d.luau`: the host's own declarations
 * (`types/generated/vos.d.luau`, or `VOS_DEFINITIONS`) with
 *
 *   - each app's types as global types under its id: `app1_Item`, `app1_AddParams`, `app1_AddResult` (as the host names
 *     its own, `vos_app_Context`); the app's own every method, a dependency's its public ones;
 *   - the four functions of `RPC` overloaded by app id and method, so a wrong app, method, params or result is a
 *     type error.
 *
 * Nothing here runs and nothing is required: the backend calls the globals, and the host's bus holds every request and
 * notification to the contract whatever the types say. Check a backend with `luau-lsp analyze
 * --definitions:@vos=types/vos.d.luau server/main.luau`.
 *
 *     RPC.on_request("add", function(params: app1_AddParams, ctx: vos_app_Context): app1_AddResult ... end)
 *     local item, err = RPC.send_request("app1", "add", { text = "milk" })   -- item: app1_AddResult?
 */
import { paramsObject, pascal, type Contract, type Method, type Schema } from './contract.ts'
import type { Dependency } from './emit-ts.ts'

export const LUAU_KEYWORDS = new Set([
  'and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for', 'function', 'if', 'in', 'local', 'nil', 'not', 'or',
  'repeat', 'return', 'then', 'true', 'until', 'while'
])

export function luauString(s: string): string {
  let out = '"'
  for (const ch of s) {
    const code = ch.codePointAt(0) as number
    if (ch === '\\') out += '\\\\'
    else if (ch === '"') out += '\\"'
    else if (ch === '\n') out += '\\n'
    else if (ch === '\r') out += '\\r'
    else if (ch === '\t') out += '\\t'
    else if (code < 0x20 || code === 0x7f) out += `\\${code}`
    else out += ch
  }
  return `${out}"`
}

const luauDoc = (text: string | undefined, indent: string): string =>
  text === undefined ? '' : `${text.split(/\r?\n/).map((line) => (line === '' ? `${indent}---` : `${indent}--- ${line}`)).join('\n')}\n`

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/
/** A field name as a Luau table writes it: bare, or `["end"]`. */
const key = (name: string): string => (IDENTIFIER.test(name) && !LUAU_KEYWORDS.has(name) ? name : `[${luauString(name)}]`)

const optional = (type: string): string => (type.endsWith('?') ? type : /^[\w.]+$/.test(type) ? `${type}?` : `(${type})?`)

/** The Luau type of a schema; `prefix` names another app's types (`App1_Item`): Luau has no namespaces. */
export function luauType(schema: Schema, prefix = ''): string {
  switch (schema.kind) {
    case 'string':
      return schema.enum ? schema.enum.map(luauString).join(' | ') : 'string'
    case 'number':
      return 'number'
    case 'boolean':
      return 'boolean'
    case 'null':
      return 'nil'
    case 'array':
      return `{ ${luauType(schema.items, prefix)} }`
    case 'object': {
      const fields = Object.entries(schema.properties).map(([name, property]) => {
        const type = luauType(property, prefix)
        return `${key(name)}: ${schema.required.includes(name) ? type : optional(type)}`
      })
      return fields.length === 0 ? '{ [string]: never }' : `{ ${fields.join(', ')} }`
    }
    case 'ref':
      return `${prefix}${schema.name}`
    case 'nullable':
      return optional(luauType(schema.inner, prefix))
  }
}

const docOf = (m: Method): string | undefined => [m.summary, m.description].filter((t) => t !== undefined).join('\n\n') || undefined

/** An app's global types are named under its id: `my-app` is `my_app_<Type>`. */
export const typePrefix = (id: string): string => `${id.replace(/-/g, '_')}_`

/** An app this backend knows: its own (every method) or a dependency (its public ones). */
interface Known {
  id: string
  contract: Contract
  own: boolean
}

const visible = (known: Known, methods: Method[]): Method[] => methods.filter((m) => known.own || !m.private)

/** An app's types, as global types under its prefix. */
function appTypes(known: Known): string[] {
  const p = typePrefix(known.id)
  const out: string[] = [`-- ===== the app ${known.id}${known.own ? ' (this one)' : ''} =====`, '']
  for (const [name, schema] of Object.entries(known.contract.components)) out.push(`${luauDoc(schema.description, '')}export type ${p}${name} = ${luauType(schema, p)}`)
  for (const m of visible(known, [...known.contract.requests, ...known.contract.notifications])) {
    const doc = luauDoc(docOf(m), '')
    if (m.params.length > 0) out.push(`${doc}export type ${p}${pascal(m.name)}Params = ${luauType(paramsObject(m), p)}`)
    if (m.kind === 'request') out.push(`${m.params.length > 0 ? '' : doc}export type ${p}${pascal(m.name)}Result = ${luauType(m.result, p)}`)
  }
  out.push('')
  return out
}

const paramsOf = (id: string, m: Method): string => (m.params.length > 0 ? `${typePrefix(id)}${pascal(m.name)}Params` : '{}')

/** The four functions of `RPC`, overloaded for the apps this backend knows; `undefined` keeps the host's own. */
function busFunctions(apps: Known[]): Record<'send_request' | 'on_request' | 'send_notification' | 'on_notification', string | undefined> {
  const own = apps.find((a) => a.own)
  const join = (items: string[]): string | undefined => (items.length === 0 ? undefined : items.join(' & '))
  const send = apps.flatMap((a) =>
    visible(a, a.contract.requests).map(
      (r) =>
        `((app: ${luauString(a.id)}, method: ${luauString(r.name)}, params: ${paramsOf(a.id, r)}${r.params.length > 0 ? '' : '?'}) -> (${typePrefix(a.id)}${pascal(r.name)}Result?, vos_rpc_RequestError?))`
    )
  )
  const serve = own
    ? own.contract.requests.map(
        (r) =>
          `((method: ${luauString(r.name)}, handler: (params: ${paramsOf(own.id, r)}, ctx: vos_app_Context) -> ${typePrefix(own.id)}${pascal(r.name)}Result) -> ())`
      )
    : []
  const notify = own
    ? own.contract.notifications.map((n) => `((name: ${luauString(n.name)}, params: ${paramsOf(own.id, n)}${n.params.length > 0 ? '' : '?'}) -> ())`)
    : []
  const hear = apps.flatMap((a) =>
    visible(a, a.contract.notifications).map(
      (n) => `((app: ${luauString(a.id)}, name: ${luauString(n.name)}, listener: (params: ${paramsOf(a.id, n)}, ctx: vos_app_Context) -> ()) -> ())`
    )
  )
  return { send_request: join(send), on_request: join(serve), send_notification: join(notify), on_notification: join(hear) }
}

/**
 * `types/vos.d.luau`: the host's declarations (`host`, the text of its `vos.d.luau`), with this app's and its
 * dependencies' types, and the global `RPC`'s functions typed for them.
 */
export function emitLuauDefinitions(host: string, appId: string, contract: Contract, dependencies: Dependency[]): string {
  const apps: Known[] = [{ id: appId, contract, own: true }, ...dependencies.map((d) => ({ id: d.id, contract: d.contract, own: false }))]
  let text = host
  for (const [name, overloads] of Object.entries(busFunctions(apps))) {
    if (overloads === undefined) continue
    // the host declares each function of `vos_rpc` on one line: `\t<name>: (...) -> ...,`
    const line = new RegExp(`^(\t${name}: ).*,$`, 'm')
    if (!line.test(text)) throw new Error(`the host's declarations have no \`RPC.${name}\` to type: they are older than this plugin`)
    text = text.replace(line, (_all, start: string) => `${start}${overloads},`)
  }
  const types = apps.flatMap(appTypes)
  return [
    `-- GENERATED by @vos/vite-plugin: do not edit. The host's declarations, typed for ${appId} and the apps it depends on.`,
    '-- Check the backend with `luau-lsp analyze --definitions:@vos=types/vos.d.luau server/main.luau`. Nothing here runs.',
    '',
    ...types,
    text
  ].join('\n')
}
