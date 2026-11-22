/** What the host gives a client (`mount(el, { rpc, host })`). */
export type Rpc = (method: string, args?: unknown) => Promise<unknown>
export interface Host {
  /** Hear an event (one the backend sent with `app.emit`, or the host's own `host:*`); returns a stop function. */
  on(name: string, fn: (data: unknown) => void): () => void
  readonly theme: 'light' | 'dark'
}
