/** How something in Docker reads at a glance: going, not going, in between, or broken. */
export type Tone = 'on' | 'off' | 'busy' | 'bad'

/** Whether Docker can be reached, as the backend's `engine.status()` answers: its version when it can, why not when it cannot. */
export interface Engine {
  reachable: boolean
  version: string
  /** The address that is chosen; empty for this machine's own Docker. */
  host: string
  problem: string
}

/** The end of what a container wrote, as the backend's `engine.logs()` answers. `truncated`: there was more before it. */
export interface Logs {
  text: string
  truncated: boolean
}
