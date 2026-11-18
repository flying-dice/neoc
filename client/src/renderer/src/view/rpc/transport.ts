/**
 * What the session needs from a socket, so the same code runs over a WebSocket (production) and over the
 * in-memory wire of the fake host (tests, Storybook).
 */
export type TransportStatus = 'connecting' | 'open' | 'closed'

export interface Transport {
  readonly status: TransportStatus
  /** Send one text frame. Throws if the transport is not open. */
  send(message: string): void
  /** Start connecting (idempotent). A closing transport that was not `close()`d reconnects by itself. */
  connect(): void
  /** Close for good: no reconnect. */
  close(): void
  /** Ask a waiting transport to retry now (the browser came back online, the user pressed "Retry"). */
  retryNow(): void
  onMessage(listener: (message: string) => void): () => void
  onStatus(listener: (status: TransportStatus) => void): () => void
}

/** A tiny listener set shared by the transports. */
export class Emitter<T> {
  private listeners = new Set<(value: T) => void>()
  on(listener: (value: T) => void): () => void {
    this.listeners.add(listener)
    return () => void this.listeners.delete(listener)
  }
  emit(value: T): void {
    for (const l of [...this.listeners]) l(value)
  }
}
