/**
 * The host context an app is told: MCP Apps' `hostContext` (theme, display mode, locale, time zone, ...), drawn from the
 * system settings. One function builds it for the shell and for the standalone wrapper, so an app sees the same
 * thing in both; they differ only in what the page can do about the display mode.
 */

export type DisplayMode = 'inline' | 'fullscreen' | 'pip'

/** The fields of MCP Apps' `HostContext` that vos fills in. */
export interface HostContext {
  theme: 'light' | 'dark'
  displayMode: DisplayMode
  availableDisplayModes: DisplayMode[]
  /** BCP 47, as the browser reports it. */
  locale: string
  /** IANA, as the browser reports it. */
  timeZone: string
  platform: 'web'
  userAgent: string
}

/** What the page decides: the appearance the system settings resolve to, and which display modes it can show. */
export interface ContextInputs {
  appearance: 'light' | 'dark'
  displayMode: DisplayMode
  availableDisplayModes: DisplayMode[]
}

export function hostContextOf(inputs: ContextInputs): HostContext {
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone
  return {
    theme: inputs.appearance,
    displayMode: inputs.displayMode,
    availableDisplayModes: inputs.availableDisplayModes,
    locale: typeof navigator === 'undefined' ? 'en-US' : navigator.language || 'en-US',
    timeZone: timeZone || 'UTC',
    platform: 'web',
    userAgent: typeof navigator === 'undefined' ? '' : navigator.userAgent
  }
}

/** The fields of `after` that differ from `before`: what `ui/notifications/host-context-changed` carries. */
export function contextChange(before: HostContext, after: HostContext): Partial<HostContext> {
  const change: Partial<HostContext> = {}
  for (const key of Object.keys(after) as (keyof HostContext)[]) {
    if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
      Object.assign(change, { [key]: after[key] })
    }
  }
  return change
}
