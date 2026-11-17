/**
 * Main side of the host protocol: exhaustive, validated IPC registration and
 * a typed event emitter.
 *
 *  • Every invoke/send channel must have a handler (missing ones are a type
 *    error), arguments are parsed with the protocol's zod schemas, and only
 *    messages from trusted renderer frames are accepted.
 */
import type { IpcMain, IpcMainEvent, IpcMainInvokeEvent, WebContents, WebFrameMain } from 'electron'
import {
  invokeChannels,
  invokeSchemas,
  sendChannels,
  sendSchemas,
  wire,
  type EventChannel,
  type EventContract,
  type InvokeChannel,
  type InvokeContract,
  type SendChannel,
  type SendContract
} from './protocol'

export type InvokeHandlers = {
  [C in InvokeChannel]: (
    ...args: InvokeContract[C]['args']
  ) => InvokeContract[C]['result'] | Promise<InvokeContract[C]['result']>
}

export type SendHandlers = {
  [C in SendChannel]: (...args: SendContract[C]) => void
}

export interface HostIpcOptions {
  /** Return true only for frames allowed to talk to main (see `trustedFrames`). */
  isTrustedSender: (frame: WebFrameMain | null) => boolean
}

export class UntrustedSenderError extends Error {
  constructor(channel: string) {
    super(`Rejected IPC "${channel}" from an untrusted frame`)
  }
}

/** Registers every protocol channel. Returns a function that unregisters them. */
export function registerHostIpc(
  ipcMain: IpcMain,
  handlers: { invoke: InvokeHandlers; send: SendHandlers },
  options: HostIpcOptions
): () => void {
  for (const channel of invokeChannels) {
    ipcMain.handle(wire(channel), (event: IpcMainInvokeEvent, ...raw: unknown[]) => {
      if (!options.isTrustedSender(event.senderFrame)) throw new UntrustedSenderError(channel)
      const args = invokeSchemas[channel].parse(raw) as InvokeContract[typeof channel]['args']
      const handler = handlers.invoke[channel] as (...a: unknown[]) => unknown
      return handler(...args)
    })
  }
  const listeners = sendChannels.map((channel) => {
    const listener = (event: IpcMainEvent, ...raw: unknown[]): void => {
      if (!options.isTrustedSender(event.senderFrame)) {
        console.warn(new UntrustedSenderError(channel).message)
        return
      }
      const parsed = sendSchemas[channel].safeParse(raw)
      if (!parsed.success) {
        console.warn(`[agentos] invalid "${channel}" payload:`, parsed.error.message)
        return
      }
      const handler = handlers.send[channel] as (...a: unknown[]) => void
      handler(...(parsed.data as unknown[]))
    }
    ipcMain.on(wire(channel), listener)
    return [channel, listener] as const
  })
  return () => {
    for (const channel of invokeChannels) ipcMain.removeHandler(wire(channel))
    for (const [channel, listener] of listeners) ipcMain.removeListener(wire(channel), listener)
  }
}

/** Typed main → renderer events. */
export type HostEmitter = <C extends EventChannel>(channel: C, payload: EventContract[C]) => void

export function createHostEmitter(target: () => WebContents | undefined): HostEmitter {
  return (channel, payload) => {
    const contents = target()
    if (contents && !contents.isDestroyed()) contents.send(wire(channel), payload)
  }
}

/**
 * Trust policy: only top-level frames whose origin is the renderer's own —
 * the dev server origin, or the packaged `file://` renderer.
 */
export function trustedFrames(rendererUrl: string | undefined): (frame: WebFrameMain | null) => boolean {
  const devOrigin = rendererUrl ? new URL(rendererUrl).origin : null
  return (frame) => {
    if (!frame || frame.parent) return false
    try {
      const url = new URL(frame.url)
      return devOrigin ? url.origin === devOrigin : url.protocol === 'file:'
    } catch {
      return false
    }
  }
}
