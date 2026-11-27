type Container = VosApp.Containers.Container
type Port = VosApp.Containers.Port
type Spec = VosApp.Containers.Spec
type Stack = VosApp.Containers.Stack
type State = VosApp.Containers.State
import type { Tone } from '@vos/docker'

const TONES: Readonly<Record<State, Tone>> = {
  running: 'on',
  created: 'off',
  exited: 'off',
  paused: 'busy',
  restarting: 'busy',
  removing: 'busy',
  dead: 'bad'
}

const LABELS: Readonly<Record<State, string>> = {
  running: 'Running',
  created: 'Created',
  exited: 'Stopped',
  paused: 'Paused',
  restarting: 'Restarting',
  removing: 'Being removed',
  dead: 'Dead'
}

export const toneOf = (state: State): Tone => TONES[state]
export const stateLabel = (state: State): string => LABELS[state]

/** A container that can be stopped: it is going, or trying to. */
export const isUp = (container: Container): boolean =>
  container.state === 'running' || container.state === 'restarting' || container.state === 'paused'

/** How much of a stack is running, in words. */
export function stackSummary(stack: Stack): string {
  if (stack.running === 0) return 'Stopped'
  if (stack.running === stack.total) return stack.total === 1 ? 'Running' : `All ${stack.total} running`
  return `${stack.running} of ${stack.total} running`
}

/** How a stack as a whole reads: all of it going, none of it, or part. */
export const stackTone = (stack: Stack): Tone => (stack.running === 0 ? 'off' : stack.running === stack.total ? 'on' : 'busy')

/** An image as a person names it: an untagged one is a digest, shown short. */
export function imageLabel(image: string): string {
  const digest = /^sha256:([0-9a-f]{12})[0-9a-f]*$/.exec(image)
  return digest ? digest[1]! : image
}

/** How many of a list of containers are running, in words. */
export function runningSummary(containers: readonly Container[]): string {
  if (containers.length === 0) return 'No containers'
  const running = containers.filter((c) => c.state === 'running').length
  const noun = containers.length === 1 ? 'container' : 'containers'
  return `${running} of ${containers.length} ${noun} running`
}

/** A port as a person reads it: the machine's address and port (or that Docker chooses one), then the container's. */
export function portLabel(port: Port): string {
  const host = port.host_port.trim() === '' ? 'any port' : port.host_port.trim()
  const ip = port.host_ip.trim()
  const on = ip === '' ? host : `${ip.includes(':') ? `[${ip}]` : ip}:${host}`
  return `${on} → ${port.container_port.trim()}/${port.protocol}`
}

/** When Docker starts a container again, in words: its policy, and how many times at most for `on-failure`. */
export function restartLabel(spec: Pick<Spec, 'restart' | 'retries'>): string {
  const times = Number(spec.retries)
  if (spec.restart !== 'on-failure' || !(times > 0)) return spec.restart
  return `on-failure, at most ${times} ${times === 1 ? 'time' : 'times'}`
}

/** A number of bytes as a person reads it: `512 B`, `12.3 MB`. */
export function bytes(count: number): string {
  const units = ['B', 'kB', 'MB', 'GB', 'TB']
  let n = Math.max(0, count)
  let unit = 0
  while (n >= 1000 && unit < units.length - 1) {
    n /= 1000
    unit += 1
  }
  return `${unit === 0 || n >= 100 ? Math.round(n) : n.toFixed(1)} ${units[unit]}`
}
