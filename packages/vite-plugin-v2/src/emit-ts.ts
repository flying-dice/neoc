/**
 * Types for the page's ambient API: the global `vos` that the host's bridge script (`/apps/bridge.js`) defines. Nothing
 * here runs: an app's page works the same without these files, they only type it.
 *
 *   - `types/vos.d.ts`: the global `vos` itself (the bus's four verbs, `vos.host`, `vos.tracing`, `vos.RpcError`),
 *     typed over two interfaces the apps fill in: `VosApps` (each app's requests and notifications, by id) and
 *     `VosSelf` (which app this page is);
 *   - `types/<id>.d.ts`: one per app the page knows, its own and each it depends on (`vos({ dependencies })`). Each
 *     adds itself to `VosApps` and its types to the namespace `VosApp.<Id>`; another app's private (`_name`) methods are
 *     left out.
 *
 *     const item = await vos.sendRequest('app1', 'add', { text: 'milk' })   // VosApp.App1.Item
 *     vos.onRequest('_client_confirm', ({ question }, { signal }) => ask(question, signal))
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

export function tsType(schema: Schema, components: Record<string, Schema>): string {
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
      const item = tsType(schema.items, components)
      return /[ |&{]/.test(item) ? `(${item})[]` : `${item}[]`
    }
    case 'object': {
      const fields = Object.entries(schema.properties).map(([name, property]) => {
        const optional = !schema.required.includes(name) || allowsNull(property, components)
        return `${key(name)}${optional ? '?' : ''}: ${tsType(property, components)}`
      })
      return fields.length === 0 ? '{ [key: string]: never }' : `{ ${fields.join('; ')} }`
    }
    case 'ref':
      return schema.name
    case 'nullable':
      return `${tsType(schema.inner, components)} | null`
  }
}

const docOf = (m: Method): string | undefined => [m.summary, m.description].filter((t) => t !== undefined).join('\n\n') || undefined

/** The types of a contract: one per component, and `<Method>Params` / `<Request>Result` for the methods given. */
function contractTypes(contract: Contract, methods: Method[]): string[] {
  const out: string[] = []
  for (const [name, schema] of Object.entries(contract.components)) out.push(`${jsDoc(schema.description, '')}type ${name} = ${tsType(schema, contract.components)}`)
  for (const m of methods) {
    if (m.params.length > 0) {
      const fields = m.params.map((p) => {
        const optional = !p.required || allowsNull(p.schema, contract.components)
        return `${jsDoc(p.description, '  ')}  ${key(p.name)}${optional ? '?' : ''}: ${tsType(p.schema, contract.components)}`
      })
      out.push(`interface ${pascal(m.name)}Params {\n${fields.join('\n')}\n}`)
    }
    if (m.kind === 'request') out.push(`type ${pascal(m.name)}Result = ${tsType(m.result, contract.components)}`)
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
  out.push(`// GENERATED by @vos/vite-plugin-v2: do not edit. The app ${id}${self ? ', this page\'s own' : ', as this app depends on it'}.`)
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

/** `types/vos.d.ts`: the global `vos`, the same for every app; typed over `VosApps` and `VosSelf`. */
export function emitVosTypes(): string {
  return `// GENERATED by @vos/vite-plugin-v2: do not edit. The page's ambient API: the global \`vos\` the host's bridge defines.

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
  onContext(fn: (context: { readonly [key: string]: unknown }) => void): () => void
  /** Run \`fn\` before the host removes the page; it may return a promise. */
  onTeardown(fn: () => void | Promise<void>): () => void
  /** Open a link outside the app: http and https only. */
  openLink(url: string): Promise<void>
  /** Ask for a display mode; resolves with the mode the host ended in. */
  requestDisplayMode(mode: 'inline' | 'fullscreen' | 'pip'): Promise<'inline' | 'fullscreen' | 'pip'>
}

type VosLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error'
type VosFields = { readonly [key: string]: unknown }

/** A span: a named stretch of the page's work; a request made in it is a child of it. */
interface VosSpan {
  /** Record \`value\` under \`key\` on the span. */
  record(key: string, value: unknown): void
  /** Run \`body\` in the span and end it when \`body\` returns, or when the promise it returns settles. */
  inScope<T>(body: () => T): T
  /** End the span now, if \`inScope\` did not. */
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
}

/** The page's whole API: the RPC bus (the same four verbs an app's backend has), the host and tracing. */
declare const vos: {
  /** Send a request to an app (this one's id too): its backend or an open page answers, the first to answer wins. */
  sendRequest<A extends keyof VosApps, M extends keyof VosRequests<A>>(
    app: A,
    method: M,
    ...params: VosArgs<VosParams<VosRequests<A>[M]>>
  ): Promise<VosResult<VosRequests<A>[M]>>
  /**
   * Serve one of this app's methods from the page, beside its backend: every handler registered for it is asked and the
   * first answer wins; \`signal\` aborts when another answered first. A public method's handler is an agent tool while
   * it is registered. Returns a function that stops serving.
   */
  onRequest<M extends keyof VosRequests<VosOwnId>>(
    method: M,
    handler: (
      params: VosHandlerParams<VosParams<VosRequests<VosOwnId>[M]>>,
      context: { signal: AbortSignal }
    ) => VosResult<VosRequests<VosOwnId>[M]> | Promise<VosResult<VosRequests<VosOwnId>[M]>>
  ): () => void
  /** Send one of this app's notifications: to its pages and backend and, unless \`_name\`, other apps that listen. */
  sendNotification<N extends keyof VosNotifications<VosOwnId>>(name: N, ...params: VosArgs<VosParams<VosNotifications<VosOwnId>[N]>>): Promise<void>
  /** Hear an app's notification (this one's, or another app's public one); returns a function that stops listening. */
  onNotification<A extends keyof VosApps, N extends keyof VosNotifications<A>>(
    app: A,
    name: N,
    handler: (params: VosParams<VosNotifications<A>[N]>) => void
  ): () => void
  readonly host: VosHost
  readonly tracing: VosTracing
  readonly RpcError: new (code: number, message: string, data?: unknown) => VosRpcError
}
`
}
