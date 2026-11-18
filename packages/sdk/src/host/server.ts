/**
 * Server side of the host protocol: an exhaustive, validated dispatcher that
 * plugs into a JSON-RPC peer (one per client connection).
 *
 *  • Every invoke/send channel must have a handler: a missing one is a type error.
 *  • Arguments are parsed with the protocol's zod schemas before any handler runs;
 *    bad input is answered with InvalidParams, unknown methods with MethodNotFound.
 *  • Events are typed: `HostEmitter` only accepts real channels and payloads.
 */
import { z } from 'zod'
import { RpcError, RpcErrorCode } from '../rpc'
import {
  invokeSchemas,
  sendSchemas,
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

/** Typed server → client events. */
export type HostEmitter = <C extends EventChannel>(channel: C, payload: EventContract[C]) => void

const isInvoke = (m: string): m is InvokeChannel => Object.hasOwn(invokeSchemas, m)
const isSend = (m: string): m is SendChannel => Object.hasOwn(sendSchemas, m)

function parse<T>(schema: z.ZodType<T>, method: string, params: unknown[]): T {
  const result = schema.safeParse(params)
  if (!result.success) {
    throw new RpcError(RpcErrorCode.InvalidParams, `Invalid params for "${method}": ${z.prettifyError(result.error)}`)
  }
  return result.data
}

/** Plug the result into `createRpcPeer({ onRequest, onNotification })`. */
export function createProtocolDispatcher(handlers: { invoke: InvokeHandlers; send: SendHandlers }): {
  onRequest(method: string, params: unknown[]): Promise<unknown>
  onNotification(method: string, params: unknown[]): void
} {
  return {
    async onRequest(method, params) {
      if (!isInvoke(method)) throw new RpcError(RpcErrorCode.MethodNotFound, `Unknown method "${method}"`)
      const args = parse(invokeSchemas[method] as z.ZodType<unknown[]>, method, params)
      const handler = handlers.invoke[method] as (...a: unknown[]) => unknown
      return handler(...args)
    },
    onNotification(method, params) {
      if (!isSend(method)) throw new RpcError(RpcErrorCode.MethodNotFound, `Unknown notification "${method}"`)
      const args = parse(sendSchemas[method] as z.ZodType<unknown[]>, method, params)
      const handler = handlers.send[method] as (...a: unknown[]) => void
      handler(...args)
    }
  }
}
