/**
 * Type-level contract: what the shell reads of `types/generated/vos.ts` is checked against the generated types
 * (`bun run typecheck` runs `tsc -p types`). The `@ts-expect-error` lines prove that what the host no longer sends
 * or accepts is rejected: if the generated types stop constraining them, the directive is unused and tsc fails.
 */
import {
  CLIENT_METHODS,
  HOST_METHODS,
  KIT_VERSION,
  PROTO_VERSION,
  type AgentList,
  type AppEvent,
  type AppList,
  type AppNotification,
  type AppPreview,
  type AppSummary,
  type Catalog,
  type DeviceList,
  type JsonValue,
  type MethodName,
  type Methods,
  type RpcError,
  type SessionHello,
  type SessionWelcome,
  type WindowOpen
} from '../generated/vos'

const proto: number = PROTO_VERSION
const kit: string = KIT_VERSION

const hello: SessionHello = { proto, kit, locale: 'en-GB', timeZone: 'Europe/London', features: [] }
const back: SessionHello = { ...hello, resume: { sessionId: 'c1', views: [] } }
const welcome: SessionWelcome = { proto: 1, kit: '1.0', hostVersion: '0.1.0', sessionId: 'c1', resumed: false }

const app: AppSummary = {
  id: 'calculator',
  name: 'Calculator',
  version: '0.1.0',
  state: 'running',
  window: { w: 340, h: 560, minW: 320, minH: 500 },
  source: 'this computer',
  permissions: ['clipboard-write']
}

// every app has a page: there is no flag that says so
const flagged: AppSummary = {
  ...app,
  // @ts-expect-error `client` is not a field of an app's row any more
  client: true
}

// @ts-expect-error an app is running, disabled or failed
const paused: AppSummary = { ...app, state: 'paused' }

const apps: AppList = { apps: [app, { ...app, state: 'failed', message: 'app `oldnotes` has no `ui` in its manifest' }] }
const preview: AppPreview = {
  url: 'file:///apps/calculator/app-ref.json',
  sha256: '0'.repeat(64),
  id: 'calculator',
  name: 'Calculator',
  version: '0.1.0',
  source: 'this computer',
  installed: { version: '0.1.0', state: 'running' }
}
const catalog: Catalog = { apps: [preview], errors: [] }
const devices: DeviceList = { devices: [{ id: 'd1', name: 'Safari on iPad', createdAt: 1, lastSeenAt: 2, current: true }] }
const agents: AgentList = {
  agents: [
    {
      id: 'claude',
      name: 'Claude',
      role: 'Pair',
      grants: { all: false, apps: ['calculator'] }
    }
  ]
}

const data: JsonValue = { changed: [1, 2, 3] }
const event: AppEvent = { app: 'notes', name: 'notes.changed', data }
const open: WindowOpen = { app: 'calculator', window: 'main', params: { expression: '1+1' } }
const note: AppNotification = { app: 'notes', title: 'Note added', body: 'from the agent' }
const failure: RpcError = { code: -32010, message: 'No note with id 99' }

// the method table: the handshake and what the host tells a shell without being asked
const methods: MethodName[] = ['session.hello', 'session.bye', 'window.open', 'notification.deliver']
const helloParams: Methods['session.hello']['params'] = hello
const helloResult: Methods['session.hello']['result'] = welcome
const opened: Methods['window.open']['params'] = open
const delivered: Methods['notification.deliver']['params'] = note
const fromClient: readonly MethodName[] = CLIENT_METHODS
const fromHost: readonly MethodName[] = HOST_METHODS

// @ts-expect-error the host draws no windows: the view protocol is not in the table
const gone: MethodName = 'view.open'

export const sample = {
  back,
  flagged,
  paused,
  apps,
  catalog,
  devices,
  agents,
  event,
  failure,
  methods,
  helloParams,
  helloResult,
  opened,
  delivered,
  fromClient,
  fromHost,
  gone
}
