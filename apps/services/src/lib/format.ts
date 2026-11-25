import type { Manager, Restart, Service, State } from '@services/rpc'

/** What a person reads of a state: words, never colour alone. */
export const stateLabel = (state: State): string =>
  ({ running: 'Running', stopped: 'Stopped', failed: 'Failed', starting: 'Starting', stopping: 'Stopping', paused: 'Paused', unknown: 'Unknown' })[state]

export type Tone = 'on' | 'off' | 'busy' | 'bad'
/** The colour of the dot beside a state's words: an empty ring for a service that is not going. */
const TONES: Readonly<Record<State, Tone>> = { running: 'on', stopped: 'off', failed: 'bad', starting: 'busy', stopping: 'busy', paused: 'busy', unknown: 'off' }
export const toneOf = (state: State): Tone => TONES[state]

/** A service that is going: the button beside it stops it. */
export const isUp = (service: Service): boolean => service.state === 'running' || service.state === 'starting'

export const restartLabel = (restart: Restart): string => ({ never: 'Never', on_failure: 'On failure', always: 'Always' })[restart]

export type Filter = 'all' | 'running' | 'stopped' | 'failed' | 'autostart'
export const FILTERS: readonly { id: Filter; label: string; icon: string }[] = [
  { id: 'all', label: 'All Services', icon: 'gearshape.2' },
  { id: 'running', label: 'Running', icon: 'play' },
  { id: 'stopped', label: 'Stopped', icon: 'stop' },
  { id: 'failed', label: 'Failed', icon: 'exclamationmark.triangle' },
  { id: 'autostart', label: 'Start Automatically', icon: 'arrow.clockwise' }
]

export const matches = (service: Service, filter: Filter): boolean => {
  switch (filter) {
    case 'all':
      return true
    case 'running':
      return isUp(service)
    case 'stopped':
      return service.state === 'stopped'
    case 'failed':
      return service.state === 'failed'
    case 'autostart':
      return service.autostart === true
  }
}

/** How many services each filter shows. */
export const counts = (services: readonly Service[]): Record<Filter, number> => ({
  all: services.length,
  running: services.filter((s) => matches(s, 'running')).length,
  stopped: services.filter((s) => matches(s, 'stopped')).length,
  failed: services.filter((s) => matches(s, 'failed')).length,
  autostart: services.filter((s) => matches(s, 'autostart')).length
})

/** The services that contain the words typed, in their label or their description. */
export const search = (services: readonly Service[], text: string): Service[] => {
  const needle = text.trim().toLowerCase()
  if (needle === '') return [...services]
  return services.filter((s) => s.label.toLowerCase().includes(needle) || (s.description ?? '').toLowerCase().includes(needle))
}

/** `3 running of 12`, as the list's summary. */
export const summary = (services: readonly Service[]): string => {
  const running = services.filter((s) => s.state === 'running').length
  return services.length === 0 ? 'No services' : `${running} running of ${services.length}`
}

/**
 * How a setting that may be missing is said. A manager that has no word for it (launchd has no description) is
 * "Not Supported"; one that has, and did not say, is "Not Available".
 */
export const unsaid = (manager: Manager, field: string): string => (manager.reported.includes(field) ? 'Not Available' : 'Not Supported')
