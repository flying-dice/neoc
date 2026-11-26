type Server = VosApp.Pages.Server
type Settings = VosApp.Pages.Settings
type Site = VosApp.Pages.Site
import type { Tone } from '@vos/docker'

/** A site is on and nothing is known to be wrong with its folder: it is one the container is made for. */
export const isServed = (site: Site): boolean => site.enabled && site.problem === ''

/** How many sites the container is made for. */
export const servedCount = (sites: readonly Site[]): number => sites.filter(isServed).length

/** The server runs, made from the settings and the sites as they stand. */
export const isServing = (server: Server): boolean => server.state === 'running' && server.current

export function serverTone(server: Server, sites: readonly Site[]): Tone {
  if (servedCount(sites) === 0) return server.state === 'absent' ? 'off' : 'busy'
  if (isServing(server)) return 'on'
  return server.state === 'running' || server.state === 'restarting' ? 'busy' : 'bad'
}

/** How the server is doing, in words. */
export function serverLabel(server: Server, sites: readonly Site[]): string {
  const served = servedCount(sites)
  if (served === 0) {
    if (server.state !== 'absent') return 'Turning off'
    return server.wanted ? 'Nothing to serve' : sites.length === 0 ? 'No sites yet' : 'Every site is off'
  }
  if (isServing(server)) return served === 1 ? 'Serving 1 site' : `Serving ${served} sites`
  if (server.state === 'running') return 'Running with older settings'
  if (server.state === 'restarting') return 'Restarting'
  return 'Not running'
}

/** What is wrong with a server that is wanted on, and what is known of why. */
export interface Attention {
  heading: string
  message: string
}

/** What the person should know of a server that has sites to serve and is not serving them as configured; nothing when it is. */
export function attentionOf(server: Server, sites: readonly Site[]): Attention | null {
  if (servedCount(sites) === 0 || isServing(server)) return null
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

/** What is typed for a port, as the port it means; nothing when it is not one. */
export function portOf(typed: string): number | undefined {
  const text = typed.trim()
  if (!/^\d{1,5}$/.test(text)) return undefined
  const port = Number(text)
  return port >= 1 && port <= 65535 ? port : undefined
}

/** What is typed for the hostnames, as the list it means: names apart by commas or spaces. */
export const hostnamesOf = (typed: string): string[] => typed.split(/[\s,]+/).filter((name) => name !== '')

/** How many sites there are, in words. */
export const sitesSummary = (count: number): string => (count === 0 ? 'No sites' : count === 1 ? '1 site' : `${count} sites`)

/**
 * The name the machine Docker runs on is reached by: the one this window was opened at for this machine's own Docker
 * (or one behind a socket), the one in the address of a Docker elsewhere.
 */
export function machineOf(engineHost: string, here: string): string {
  const elsewhere = /^(?:tcp|https?):\/\/(\[[^\]]+\]|[^/:]+)/.exec(engineHost)
  return elsewhere?.[1] ?? here
}

/** One way a site is reached. */
export interface Address {
  /** As it is shown: without the scheme. */
  label: string
  url: string
}

/** Every way a site is reached: its own port on the machine, and each of its names on the shared port. */
export function addressesOf(site: Site, settings: Settings, machine: string): Address[] {
  const addresses: Address[] = []
  if (site.port > 0) addresses.push({ label: `${machine}:${site.port}`, url: `http://${machine}:${site.port}/` })
  // the web's own port is not written
  const shared = settings.port === 80 ? '' : `:${settings.port}`
  for (const name of site.hostnames) addresses.push({ label: `${name}${shared}`, url: `http://${name}${shared}/` })
  return addresses
}
