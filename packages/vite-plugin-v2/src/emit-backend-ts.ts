/**
 * Types for a TypeScript backend's ambient API: the global `RPC` the host's VM gives every app (`RPC.send_request`,
 * `RPC.on_request`, `RPC.send_notification`, `RPC.on_notification`), typed for this app and the apps it depends on, as
 * `emit-luau.ts` does for a Luau one. The output is one declarations file, `types/vos.backend.d.ts`: the host's own
 * (`types/generated/vos.d.ts`, or `VOS_DEFINITIONS`'s folder) with
 *
 *   - each app's types as global types under its id: `app1_Item`, `app1_AddParams`, `app1_AddResult` (as the host
 *     names its own, `vos_app_Context`); the app's own every method, a dependency's its public ones;
 *   - the four functions of `vos:rpc` (the module `RPC` is) declared once per app and method, as overloads, so a wrong
 *     app, method, params or result is a type error.
 *
 * Nothing here runs and nothing is imported: the backend calls the globals, and the host's bus holds every request
 * and notification to the contract whatever the types say. `tsconfig.backend.json` beside it is what `server/tsconfig.json`
 * extends, and what the build compiles with.
 *
 *     RPC.on_request('add', (params, ctx) => { ... })                    // params: app1_AddParams, result: app1_AddResult
 *     const [item, err] = RPC.send_request('app1', 'add', { text: 'milk' })   // item: app1_AddResult | undefined
 */
import { pascal, type Contract, type Method } from './contract.ts'
import { contractTypes, docOf, jsDoc, jsString, type Dependency } from './emit-ts.ts'
import { typePrefix } from './emit-luau.ts'

/** An app this backend knows: its own (every method) or a dependency (its public ones). */
interface Known {
  id: string
  contract: Contract
  own: boolean
}

const visible = (known: Known, methods: Method[]): Method[] => methods.filter((m) => known.own || !m.private)

const paramsOf = (id: string, m: Method): string => (m.params.length > 0 ? `${typePrefix(id)}${pascal(m.name)}Params` : '{ [key: string]: never }')
/** The parameter a method's params go in: absent when it has none. */
const paramsArg = (id: string, m: Method): string => (m.params.length > 0 ? `, params: ${paramsOf(id, m)}` : '')
const resultOf = (id: string, m: Method): string => `${typePrefix(id)}${pascal(m.name)}Result`

/** An app's types, as global types under its prefix. */
function appTypes(known: Known): string[] {
  const methods = visible(known, [...known.contract.requests, ...known.contract.notifications])
  return [`// ===== the app ${known.id}${known.own ? ' (this one)' : ''} =====`, '', ...contractTypes(known.contract, methods, typePrefix(known.id)), '']
}

const overload = (name: string, doc: string | undefined, signature: string): string => `${jsDoc(doc, '  ')}  export function ${name}(this: void, ${signature}`

/** The four functions of `vos:rpc`, as overloads for the apps this backend knows; `undefined` keeps the host's own. */
function busFunctions(apps: Known[]): Record<'send_request' | 'on_request' | 'send_notification' | 'on_notification', string | undefined> {
  const own = apps.find((a) => a.own)
  const lines = (items: string[]): string | undefined => (items.length === 0 ? undefined : items.join('\n'))
  const send = apps.flatMap((a) =>
    visible(a, a.contract.requests).map((r) =>
      overload(
        'send_request',
        docOf(r),
        `app: ${jsString(a.id)}, method: ${jsString(r.name)}${r.params.length > 0 ? paramsArg(a.id, r) : ', params?: { [key: string]: never }'}): LuaMultiReturn<[${resultOf(a.id, r)}, undefined] | [undefined, vos_rpc_RequestError]>`
      )
    )
  )
  const serve = own
    ? own.contract.requests.map((r) =>
        overload('on_request', docOf(r), `method: ${jsString(r.name)}, handler: (this: void, params: ${paramsOf(own.id, r)}, ctx: vos_app_Context) => ${resultOf(own.id, r)}, about?: vos_rpc_About): void`)
      )
    : []
  const notify = own
    ? own.contract.notifications.map((n) =>
        overload('send_notification', docOf(n), `name: ${jsString(n.name)}${paramsArg(own.id, n)}): void`)
      )
    : []
  const hear = apps.flatMap((a) =>
    visible(a, a.contract.notifications).map((n) =>
      overload('on_notification', docOf(n), `app: ${jsString(a.id)}, name: ${jsString(n.name)}, listener: (this: void, params: ${paramsOf(a.id, n)}, ctx: vos_app_Context) => void): void`)
    )
  )
  return { send_request: lines(send), on_request: lines(serve), send_notification: lines(notify), on_notification: lines(hear) }
}

/**
 * `types/vos.backend.d.ts`: the host's declarations (`host`, the text of its `vos.d.ts`), with this app's and its
 * dependencies' types, and the functions of `vos:rpc` typed for them.
 */
export function emitBackendTypes(host: string, appId: string, contract: Contract, dependencies: Dependency[]): string {
  const apps: Known[] = [{ id: appId, contract, own: true }, ...dependencies.map((d) => ({ id: d.id, contract: d.contract, own: false }))]
  let text = host
  for (const [name, overloads] of Object.entries(busFunctions(apps))) {
    if (overloads === undefined) continue
    // the host declares each function of `vos:rpc` on one line: `  export function <name>(...): ...`
    const line = new RegExp(`^  export function ${name}\\(.*$`, 'm')
    if (!line.test(text)) throw new Error(`the host's declarations have no \`RPC.${name}\` to type: they are older than this plugin`)
    text = text.replace(line, () => overloads)
  }
  // after the host's text, not before: its `/// <reference>` must come first in the file
  return [
    text,
    `// GENERATED by @vos/vite-plugin-v2: do not edit. The host's declarations above, typed for ${appId} and the apps it depends on.`,
    '// A TypeScript backend is checked against this file alone (tsconfig.backend.json). Nothing here runs.',
    '',
    ...apps.flatMap(appTypes)
  ].join('\n')
}

/**
 * `types/tsconfig.backend.json`: how the backend's TypeScript is checked, by the build and (through the one line of
 * `server/tsconfig.json` that extends it) by the editor. `types` is empty so that nothing of the page's world
 * (`@types/*`, the DOM) is in scope: a backend has TypeScript's own library, the host's modules and the globals `app` and
 * `RPC`. `typeRoots` is where this machine has the compiler's own types (the file is generated, never committed).
 * `module: preserve` is what lets the backend's files be plain modules.
 * @param typeRoot where `@typescript-to-lua/language-extensions` is, which the host's declarations reference
 * @param up the way from the folder this file is in (`types/`) to the project, with `/`
 */
export function emitBackendTsconfig(typeRoot: string, up: string): string {
  const config = {
    compilerOptions: {
      target: 'ESNext',
      lib: ['ESNext'],
      module: 'preserve',
      moduleResolution: 'bundler',
      strict: true,
      types: [],
      typeRoots: [typeRoot],
      noEmit: true,
      rootDir: up
    },
    include: [`${up}/server/**/*.ts`, './vos.backend.d.ts'],
    // a test beside the backend is the test runner's, not the app's
    exclude: [`${up}/server/**/*.test.ts`, `${up}/server/**/*.spec.ts`]
  }
  return `${JSON.stringify(config, null, 2)}\n`
}
