/**
 * The host services: what an app may ask of the host page that holds it, registered once and served by both host
 * pages, the shell and the standalone wrapper, from this same code.
 *
 * A service is a method of the bridge's JSON-RPC dialect (`ui/...` where MCP Apps names it, `vos/...` where it is
 * vos's own) that is data or a service and needs no host UI. The registry also says what the host offers, which is
 * what the handshake reports as `hostCapabilities` and what an app checks before using a control that needs one.
 *
 * Adding a host API is one `register` call here. The shell may register more on top for what it draws or arranges
 * (sheets, toasts, window layouts); the wrapper does not, and the capability is then absent in its handshake, so an
 * app can see that and a call to it fails with a clear message.
 */
import { HostError } from './channel'
import type { DisplayMode, HostContext } from './hostContext'
import { registerAgui } from './hostServices/agui'

/** MCP Apps' `HostCapabilities`, with vos's own under `experimental.vos`. */
export interface HostCapabilities {
  openLinks?: Record<string, never>
  experimental?: { vos?: VosCapabilities }
}

/** What vos adds to the dialect. `ui` is present only when the host page draws UI of its own (the shell). */
export interface VosCapabilities {
  /** The app's backend calls (any method that is not `ui/` or `vos/`) and `vos/event`. */
  backend: Record<string, never>
  events: Record<string, never>
  ui?: Record<string, never>
}

export interface ServiceContext {
  /** The app the calling frame belongs to: fixed by the page, never by the message. */
  appId: string
  /**
   * Aborted when the document that made the call goes: its window closes, the frame is replaced, or the page reloads
   * (a second `ui/initialize`). Work a service keeps going for the page (a stream) ends with it.
   */
  signal: AbortSignal
}

export type Service = (params: unknown, context: ServiceContext) => unknown

interface Registration {
  service: Service
  capability?: (capabilities: HostCapabilities) => void
}

export class ServiceRegistry {
  readonly #services = new Map<string, Registration>()

  /** Offer `method`. `capability` says what the handshake reports for it. */
  register(
    method: string,
    service: Service,
    capability?: (capabilities: HostCapabilities) => void
  ): this {
    if (this.#services.has(method))
      throw new Error(`the host service ${method} is registered twice`)
    this.#services.set(method, { service, capability })
    return this
  }

  get(method: string): Service | undefined {
    return this.#services.get(method)?.service
  }

  has(method: string): boolean {
    return this.#services.has(method)
  }

  /** What the handshake reports. */
  capabilities(): HostCapabilities {
    const capabilities: HostCapabilities = {
      experimental: { vos: { backend: {}, events: {} } }
    }
    for (const { capability } of this.#services.values()) capability?.(capabilities)
    return capabilities
  }
}

/** What the services need from the page they run in. */
export interface ServiceEnvironment {
  /** The display mode now and the modes this page can show. */
  context(): HostContext
  /** Open a link: the page's own business (a new tab). */
  openLink(url: URL): void
  /**
   * Ask the page to change its display mode; returns the mode it ended up in. A page that cannot change it (the
   * standalone wrapper is always full screen) leaves this out and answers with the mode it has.
   */
  setDisplayMode?(mode: DisplayMode): DisplayMode
}

/** The services both host pages offer. */
export function baseServices(environment: ServiceEnvironment): ServiceRegistry {
  const registry = new ServiceRegistry()
  registerAgui(registry)
  return registry
    .register(
      'ui/open-link',
      (params) => {
        const url = linkOf(params)
        environment.openLink(url)
        return {}
      },
      (capabilities) => {
        capabilities.openLinks = {}
      }
    )
    .register(
      'ui/request-display-mode',
      (params) => {
        const wanted = modeOf(params)
        const context = environment.context()
        // the specification: a mode the host cannot show is answered with the mode it has
        const mode =
          context.availableDisplayModes.includes(wanted) && environment.setDisplayMode
            ? environment.setDisplayMode(wanted)
            : context.displayMode
        return { mode }
      },
      (capabilities) => {
        // a page that arranges windows draws UI of its own: the shell does, the standalone page does not
        if (!environment.setDisplayMode) return
        capabilities.experimental = {
          vos: { backend: {}, events: {}, ...capabilities.experimental?.vos, ui: {} }
        }
      }
    )
}

/** MCP Apps' error code for a failed `ui/` request (implementation-defined). */
const IMPLEMENTATION_ERROR = -32000
const INVALID_PARAMS = -32602

function linkOf(params: unknown): URL {
  const raw =
    typeof params === 'object' && params !== null ? (params as { url?: unknown }).url : undefined
  if (typeof raw !== 'string') throw new HostError(INVALID_PARAMS, 'ui/open-link needs a url')
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    throw new HostError(IMPLEMENTATION_ERROR, `${raw.slice(0, 80)} is not a link`)
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') {
    throw new HostError(
      IMPLEMENTATION_ERROR,
      `Only http and https links open (not ${url.protocol})`
    )
  }
  return url
}

function modeOf(params: unknown): DisplayMode {
  const raw =
    typeof params === 'object' && params !== null ? (params as { mode?: unknown }).mode : undefined
  if (raw !== 'inline' && raw !== 'fullscreen' && raw !== 'pip') {
    throw new HostError(
      INVALID_PARAMS,
      'ui/request-display-mode needs a mode: inline, fullscreen or pip'
    )
  }
  return raw
}
