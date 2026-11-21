/**
 * Wire types of the vos view protocol (proto 1, kit 1.0).
 *
 * The Rust schema is the source of truth: `types/generated/vos.ts` is generated from it (`bun run codegen`).
 * This module is the renderer's only door to it: it re-exports the generated shapes under the names the renderer
 * uses, and derives the request and notification tables from the generated method table, so a wire change is a
 * compile error here and nowhere else. Nothing else in the renderer declares a message, node, op, patch or action
 * type or imports the generated module directly.
 */
import type * as G from '../../../types/generated/vos'

export { PROTO_VERSION as PROTO, KIT_VERSION as KIT } from '../../../types/generated/vos'

export type Json = G.JsonValue

// ── Expressions ─────────────────────────────────────────────────

/** A reference into a binding scope: client.* | state.* | src.<id>[.field] | row.* | route.* | env.* */
export type Ref = G.Ref
/** A call of a function from the closed set. */
export type Fn = G.FnCall
/** A literal object that happens to contain `$ref` or `$fn` as its own key. */
export type Lit = G.Lit
export type Expr = G.Expr

// ── Actions and handlers ────────────────────────────────────────

export type Action = G.Action
/** `true` = send the event to the app; an Action[] runs on the client (`emit` entries also go to the app). */
export type Handler = G.Handler

/** The `route` of a `nav.push` action and the entries of a navigation path: `{name, ...params}`. */
export type Route = { name: string; [param: string]: Json }

// ── Nodes and ops ──────────────────────────────────────────

export type Node = G.Node
export type Op = G.Op

// ── State, client state, sources ──────────────────────

export type RowKey = string | number
export type Row = { [field: string]: Json }

/** A state value with its revision (`data.patch.state`, `ViewSnapshot.state`). */
export type StateValue = G.StateValue

export type SourceDecl = G.SourceDecl
export type ClientDecl = G.ClientDecl
export type SourcePatch = G.SourcePatch
export type RowsPatch = Extract<SourcePatch, { kind: 'rows' }>
export type ViewSnapshot = G.ViewSnapshot

// ── Client -> host requests ─────────────────────────────────────

export type WindowSize = G.Size
export type ResumeDraft = G.Draft
export type ViewOpenParams = G.ViewOpen
export type SessionHelloResult = G.SessionWelcome
export type ViewEventParams = G.ViewEvent
export type ViewEventResult = G.EventAck
/** The ack of a two-way write: the authoritative value and revision. */
export type WriteResult = G.WriteAck
export type WriteResolution = G.WriteResolution
export type SourceSetParams = G.SourceSet

type Entry<K extends 'request' | 'notification', D extends 'clientToHost' | 'hostToClient'> = {
  [M in G.MethodName as G.Methods[M] extends { kind: K; direction: D } ? M : never]: G.Methods[M]
}

/** Client -> host requests: method -> params and result. */
export type RequestMethods = {
  [M in keyof Entry<'request', 'clientToHost'>]: {
    params: G.Methods[M]['params']
    result: G.Methods[M]['result']
  }
}

/** Client -> host notifications: method -> params. */
export type ClientNotifications = {
  [M in keyof Entry<'notification', 'clientToHost'>]: G.Methods[M]['params']
}

// ── Host -> client notifications ────────────────────────────────

export type SizeClass = 'compact' | 'regular'

export type DataPatch = G.DataPatch
export type ViewCommand = G.Command
export type ThemeSet = G.ThemeSet

/** Host -> client notifications: method -> params. */
export type HostNotifications = {
  [M in keyof Entry<'notification', 'hostToClient'>]: G.Methods[M]['params']
}

/** JSON-RPC application error codes. */
export const AppErrorCode = {
  /** The handler raised an error whose message is safe to show. */
  AppError: -32010
} as const

export type RequestMethod = keyof RequestMethods

// ── Host management: the shell's own calls (`app.*`, `agents.*`, `host.devices.*`) ─────────────────────────

export type HostAppState = G.AppState
export type HostAppSummary = G.AppSummary
export type HostInstalledInfo = G.InstalledInfo
export type HostAppPreview = G.AppPreview
export type HostCatalog = G.Catalog
export type HostAgentRow = G.AgentRow
export type HostAgentList = G.AgentList
export type HostDeviceRow = G.DeviceRow
