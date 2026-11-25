import type { Server, Settings } from '@dns/rpc'
import type { Tone } from '@vos/docker'

/** The server answers, with the settings and the names as they stand. */
export const isServing = (server: Server): boolean => server.state === 'running' && server.current

export function serverTone(server: Server): Tone {
  if (!server.enabled) return server.state === 'absent' ? 'off' : 'busy'
  if (isServing(server)) return 'on'
  return server.state === 'running' || server.state === 'restarting' ? 'busy' : 'bad'
}

/** How the server is doing, in words. */
export function serverLabel(server: Server, settings: Settings): string {
  if (!server.enabled) return server.state === 'absent' ? 'Off' : 'Turning off'
  if (isServing(server)) return `Answering on port ${settings.port}`
  if (server.state === 'running') return 'Running with older settings'
  if (server.state === 'restarting') return 'Restarting'
  return 'Not running'
}

/** What is wrong with a server that is wanted on, and what is known of why. */
export interface Attention {
  heading: string
  message: string
}

/** What the person should know of a server that is wanted on and is not answering as configured; nothing when it is. */
export function attentionOf(server: Server): Attention | null {
  if (!server.enabled || isServing(server)) return null
  if (server.state === 'running') {
    return {
      heading: 'The server is running with older settings',
      message: server.problem || 'Its container was made before the last change.'
    }
  }
  if (server.state === 'restarting') {
    return { heading: 'The server keeps restarting', message: server.problem || 'Its log says why.' }
  }
  return {
    heading: 'The server is not running',
    message: server.problem || (server.state === 'absent' ? 'Its container is gone.' : `Its container stopped: ${server.status || 'the log says why'}.`)
  }
}

/** What is typed for the upstream servers, as the list it means: addresses apart by commas or spaces. */
export const upstreamsOf = (typed: string): string[] => typed.split(/[\s,]+/).filter((address) => address !== '')

/** What is typed for the port, as the port it means; nothing when it is not one. */
export function portOf(typed: string): number | undefined {
  const text = typed.trim()
  if (!/^\d{1,5}$/.test(text)) return undefined
  const port = Number(text)
  return port >= 1 && port <= 65535 ? port : undefined
}

/** How many records there are, in words. */
export const recordsSummary = (count: number): string => (count === 0 ? 'No names' : count === 1 ? '1 name' : `${count} names`)
