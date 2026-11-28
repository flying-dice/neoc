/**
 * The three types of agent, and what each takes. How a chat reaches the agent is its type, fixed once it is added:
 * `remote` at its address over a WebSocket, `container` in a container of each chat's own made from an image, and
 * `program` as a command run on this machine. Every type has a name, a picture, a work folder and MCP servers.
 */
import { textOf } from './words'

type Agent = VosApp.Agents.Agent
export type Kind = VosApp.Agents.Kind

/** A type as the type picker offers it: its name, its icon and what it is, in a line. */
export interface KindInfo {
  kind: Kind
  label: string
  icon: string
  /** The colour of its icon's tile: a palette token. */
  tint: string
  explanation: string
}

export const KINDS: readonly KindInfo[] = [
  { kind: 'remote', label: 'Remote', icon: 'globe', tint: 'blue', explanation: 'An agent that runs elsewhere and is reached at its ACP address, like acpd serves.' },
  {
    kind: 'container',
    label: 'Container',
    icon: 'rectangle.stack',
    tint: 'teal',
    explanation: 'An agent run in a container of each chat’s own, made from an image built or pulled in Containers.'
  },
  { kind: 'program', label: 'Program', icon: 'terminal', tint: 'gray', explanation: 'An agent command run on this machine, in a folder of it.' }
]

/** What each type takes besides the name, picture, work folder and MCP servers every agent has. */
export interface Takes {
  /** An address and a bearer token. */
  address: boolean
  /** A command, and the environment variables it starts with. */
  command: boolean
  /** An image, a skills folder and skills. */
  image: boolean
}

export function takes(kind: Kind): Takes {
  return { address: kind === 'remote', command: kind !== 'remote', image: kind === 'container' }
}

/** The work folder a new agent of each type starts with: the backend's own for a blank one. A program's has to be chosen. */
export function folderFor(kind: Kind): string {
  return kind === 'remote' ? '/' : kind === 'container' ? '/workspace' : ''
}

/** Where an agent's sessions work, in a line: on its machine, in the container, or on this machine. */
export function folderHint(kind: Kind): string {
  return kind === 'remote'
    ? 'The folder is where its sessions work, on its own machine.'
    : kind === 'container'
      ? 'The folder is where its sessions work in the container: each chat gets a volume of its own there.'
      : 'The folder is where the program starts and its sessions work, on this machine. It must be there.'
}

/** The line under an agent's name in the sidebar: its address, that it is a container, or its command. */
export function subheadingOf(agent: Agent): string {
  return agent.kind === 'remote' ? agent.url : agent.kind === 'container' ? 'Container' : textOf(agent.command)
}

/** Whether an agent's chats run in Docker: what Docker being away stops. */
export const needsDocker = (kind: Kind | undefined): boolean => kind === 'container'
