/**
 * Types for the page's ambient API: the globals `RPC` and `vos` that the host's bridge script (`/apps/bridge.js`)
 * defines. Nothing here runs: an app's page works the same without these files, they only type it.
 *
 *   - `types/vos.d.ts`: the globals themselves, `RPC` (the bus, in the house style: `RPC.send_request`,
 *     `RPC.on_request`, `RPC.send_notification`, `RPC.on_notification`, `RPC.RpcError`) and `vos` (`vos.host`,
 *     `vos.tracing`), typed over two interfaces the apps fill in: `VosApps` (each app's requests and notifications, by id) and
 *     `VosSelf` (which app this page is);
 *   - `types/<id>.d.ts`: one per app the page knows, its own and each it depends on (`vos({ dependencies })`). Each
 *     adds itself to `VosApps` and its types to the namespace `VosApp.<Id>`; another app's private (`_name`) methods are
 *     left out.
 *
 *     const item = await RPC.send_request('app1', 'add', { text: 'milk' })   // VosApp.App1.Item
 *     RPC.on_request('_client_confirm', ({ question }, { signal }) => ask(question, signal))
 */
import { allowsNull, pascal, type Contract, type Method, type Schema } from './contract.ts'

/** An app this one depends on: its id (from its package.json) and its contract. */
export interface Dependency {
  id: string
  contract: Contract
}

export const jsString = (s: string): string => JSON.stringify(s)
export const jsDoc = (text: string | undefined, indent: string): string =>
  text === undefined ? '' : `${indent}/** ${text.replace(/\*\//g, '*\\/').replace(/\r?\n/g, `\n${indent} * `)} */\n`

const PROPERTY = /^[A-Za-z_$][A-Za-z0-9_$]*$/
const key = (name: string): string => (PROPERTY.test(name) ? name : jsString(name))

/** `my-lib` is the namespace `VosApp.MyLib` of its types. */
export const namespaceOf = (id: string): string =>
  id
    .split('-')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join('')

/** The TypeScript type of a schema; `prefix` names the types of an app that has them global (`app1_Item`). */
export function tsType(schema: Schema, components: Record<string, Schema>, prefix = ''): string {
  switch (schema.kind) {
    case 'string':
      return schema.enum ? schema.enum.map(jsString).join(' | ') : 'string'
    case 'number':
      return 'number'
    case 'boolean':
      return 'boolean'
    case 'null':
      return 'null'
    case 'array': {
      const item = tsType(schema.items, components, prefix)
      return /[ |&{]/.test(item) ? `(${item})[]` : `${item}[]`
    }
    case 'object': {
      const fields = Object.entries(schema.properties).map(([name, property]) => {
        const optional = !schema.required.includes(name) || allowsNull(property, components)
        return `${key(name)}${optional ? '?' : ''}: ${tsType(property, components, prefix)}`
      })
      return fields.length === 0 ? '{ [key: string]: never }' : `{ ${fields.join('; ')} }`
    }
    case 'ref':
      return `${prefix}${schema.name}`
    case 'nullable':
      return `${tsType(schema.inner, components, prefix)} | null`
  }
}

export const docOf = (m: Method): string | undefined => [m.summary, m.description].filter((t) => t !== undefined).join('\n\n') || undefined

/**
 * The types of a contract: one per component, and `<Method>Params` / `<Request>Result` for the methods given. With a
 * `prefix` they are global types under it (`app1_Item`, `app1_AddParams`), as the backend's declarations have them.
 */
export function contractTypes(contract: Contract, methods: Method[], prefix = ''): string[] {
  const out: string[] = []
  for (const [name, schema] of Object.entries(contract.components)) out.push(`${jsDoc(schema.description, '')}type ${prefix}${name} = ${tsType(schema, contract.components, prefix)}`)
  for (const m of methods) {
    if (m.params.length > 0) {
      const fields = m.params.map((p) => {
        const optional = !p.required || allowsNull(p.schema, contract.components)
        return `${jsDoc(p.description, '  ')}  ${key(p.name)}${optional ? '?' : ''}: ${tsType(p.schema, contract.components, prefix)}`
      })
      out.push(`interface ${prefix}${pascal(m.name)}Params {\n${fields.join('\n')}\n}`)
    }
    if (m.kind === 'request') out.push(`type ${prefix}${pascal(m.name)}Result = ${tsType(m.result, contract.components, prefix)}`)
  }
  return out
}

const indent = (lines: string[], by: string): string =>
  lines.map((line) => line.split('\n').map((l) => (l === '' ? l : `${by}${l}`)).join('\n')).join('\n')

/**
 * `types/<id>.d.ts`: an app, as this page knows it: its own (`self`: every method, and `VosSelf`) or one it depends
 * on (its public methods).
 */
export function emitAppTypes(id: string, contract: Contract, self: boolean): string {
  const ns = namespaceOf(id)
  const requests = contract.requests.filter((r) => self || !r.private)
  const notifications = contract.notifications.filter((n) => self || !n.private)
  const params = (m: Method): string => (m.params.length === 0 ? 'void' : `VosApp.${ns}.${pascal(m.name)}Params`)
  const entry: string[] = []
  entry.push(`${jsString(id)}: {`)
  entry.push('  requests: {')
  for (const r of requests) entry.push(`${jsDoc(docOf(r), '    ')}    ${jsString(r.name)}: { params: ${params(r)}; result: VosApp.${ns}.${pascal(r.name)}Result }`)
  entry.push('  }')
  entry.push('  notifications: {')
  for (const n of notifications) entry.push(`${jsDoc(docOf(n), '    ')}    ${jsString(n.name)}: { params: ${params(n)} }`)
  entry.push('  }')
  entry.push('}')
  const out: string[] = []
  out.push(`// GENERATED by @vos/vite-plugin: do not edit. The app ${id}${self ? ', this page\'s own' : ', as this app depends on it'}.`)
  out.push('')
  out.push(`/** The types of the app ${id}. */`)
  out.push('declare namespace VosApp {')
  out.push(`  namespace ${ns} {`)
  out.push(indent(contractTypes(contract, [...requests, ...notifications]), '    '))
  out.push('  }')
  out.push('}')
  out.push('')
  out.push('interface VosApps {')
  out.push(indent(entry, '  '))
  out.push('}')
  if (self) {
    out.push('')
    out.push('/** This page is the app ' + id + '\'s. */')
    out.push(`interface VosSelf {\n  id: ${jsString(id)}\n}`)
  }
  out.push('')
  return out.join('\n')
}

/** `types/vos.d.ts`: the globals `RPC` and `vos`, the same for every app; typed over `VosApps` and `VosSelf`. */
export function emitVosTypes(): string {
  return `// GENERATED by @vos/vite-plugin: do not edit. The page's ambient API: the globals \`RPC\` and \`vos\` the host's bridge defines.

/** The apps this page has types for, by id: each adds itself (\`types/<id>.d.ts\`). */
interface VosApps {}
/** Which app this page is: its own types add \`id\`. */
interface VosSelf {}

/** The params argument: none for a method without params, optional when every param is. */
type VosArgs<P> = [P] extends [void] ? [] : {} extends P ? [params?: P] : [params: P]
/** What a handler is given for a method without params. */
type VosHandlerParams<P> = [P] extends [void] ? Record<string, never> : P
/** This page's own app. */
type VosOwnId = VosSelf extends { id: infer I extends keyof VosApps } ? I : never
type VosRequests<A extends keyof VosApps> = VosApps[A] extends { requests: infer R } ? R : never
type VosNotifications<A extends keyof VosApps> = VosApps[A] extends { notifications: infer N } ? N : never
type VosParams<T> = T extends { params: infer P } ? P : never
type VosResult<T> = T extends { result: infer R } ? R : never

/** What a failed request rejects with: a JSON-RPC error. */
interface VosRpcError extends Error {
  readonly code: number
  readonly data: unknown
}

/** The host page that holds the app. */
interface VosHost {
  /** Hear an event of the host (\`host:reconnected\`); returns a function that stops listening. */
  on(name: string, fn: (data: unknown) => void): () => void
  /** The appearance the host shows now: the page follows it. */
  readonly theme: 'light' | 'dark'
  /** The host context (MCP Apps' \`hostContext\`): theme, displayMode, locale, timeZone, ... */
  readonly context: { readonly [key: string]: unknown }
  /** What the host offers (MCP Apps' \`hostCapabilities\`), known once \`ready\` has resolved. */
  readonly capabilities: { readonly [key: string]: unknown }
  /** True when the host page draws UI of its own (the shell); false on the standalone page. */
  readonly hosted: boolean
  /** Resolves when the handshake with the host is done. */
  readonly ready: Promise<void>
  /** Hear a change of the host context; returns a function that stops listening. */
  on_context(fn: (context: { readonly [key: string]: unknown }) => void): () => void
  /** Run \`fn\` before the host removes the page; it may return a promise. */
  on_teardown(fn: () => void | Promise<void>): () => void
  /** Open a link outside the app: http and https only. */
  open_link(url: string): Promise<void>
  /** Ask for a display mode; resolves with the mode the host ended in. */
  request_display_mode(mode: 'inline' | 'fullscreen' | 'pip'): Promise<'inline' | 'fullscreen' | 'pip'>
}

type VosLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error'
type VosFields = { readonly [key: string]: unknown }

/** A span: a named stretch of the page's work; a request made in it is a child of it. */
interface VosSpan {
  /** Record \`value\` under \`key\` on the span. */
  record(key: string, value: unknown): void
  /** Run \`body\` in the span and end it when \`body\` returns, or when the promise it returns settles. */
  in_scope<T>(body: () => T): T
  /** End the span now, if \`in_scope\` did not. */
  end(): void
}

/** Spans and events, after Rust's \`tracing\`. */
interface VosTracing {
  span(name: string, fields?: VosFields, options?: { level?: VosLevel }): VosSpan
  instrument<A extends unknown[], R>(name: string, body: (...args: A) => R, options?: { level?: VosLevel; fields?: (...args: A) => VosFields }): (...args: A) => R
  event(level: VosLevel, message: string | Error, fields?: VosFields): void
  trace(message: string, fields?: VosFields): void
  debug(message: string, fields?: VosFields): void
  info(message: string, fields?: VosFields): void
  warn(message: string, fields?: VosFields): void
  error(message: string | Error, fields?: VosFields): void
  /** What was recorded, for the Telemetry app: the host answers that app's frame only, and refuses every other. */
  query(params: VosFields): Promise<unknown>
}

/** What a page's handler says about itself: an agent is shown it when the method is public (a tool). */
interface VosAbout {
  title?: string
  description?: string
  /** The JSON Schema of its params. */
  input_schema?: { readonly [key: string]: unknown }
  /** It changes nothing. */
  read_only?: boolean
}

/** The RPC bus: the same four functions, in the same house style (snake case), as an app's backend has. */
declare const RPC: {
  /** Send a request to an app (this one's id too): its backend or an open page answers, the first to answer wins. */
  send_request<A extends keyof VosApps, M extends keyof VosRequests<A>>(
    app: A,
    method: M,
    ...params: VosArgs<VosParams<VosRequests<A>[M]>>
  ): Promise<VosResult<VosRequests<A>[M]>>
  /**
   * Serve one of this app's methods from the page, beside its backend: every handler registered for it is asked and the
   * first answer wins; \`signal\` aborts when another answered first. A public method's handler is an agent tool while
   * it is registered. Returns a function that stops serving.
   */
  on_request<M extends keyof VosRequests<VosOwnId>>(
    method: M,
    handler: (
      params: VosHandlerParams<VosParams<VosRequests<VosOwnId>[M]>>,
      context: { signal: AbortSignal }
    ) => VosResult<VosRequests<VosOwnId>[M]> | Promise<VosResult<VosRequests<VosOwnId>[M]>>,
    about?: VosAbout
  ): () => void
  /** Send one of this app's notifications: to its pages and backend and, unless \`_name\`, other apps that listen. */
  send_notification<N extends keyof VosNotifications<VosOwnId>>(name: N, ...params: VosArgs<VosParams<VosNotifications<VosOwnId>[N]>>): Promise<void>
  /** Hear an app's notification (this one's, or another app's public one); returns a function that stops listening. */
  on_notification<A extends keyof VosApps, N extends keyof VosNotifications<A>>(
    app: A,
    name: N,
    handler: (params: VosParams<VosNotifications<A>[N]>) => void
  ): () => void
  readonly RpcError: new (code: number, message: string, data?: unknown) => VosRpcError
}

/** The rest of the page's API: the host and tracing. */
declare const vos: {
  readonly host: VosHost
  readonly tracing: VosTracing
}
`
}
