// The host page's `vos/telemetry/query`, through the bridge's global: it is a service of the host page, not one of the
// app's own methods, so the app's contract (`openrpc.json`) does not have it. Only the composition root uses this.
import type { TelemetryReader } from './telemetry.svelte'
import type { Found, TraceSummary } from './telemetry'

interface Bridge {
  call(method: string, params?: unknown): Promise<unknown>
}

export function bridgeReader(): TelemetryReader {
  const vos = (globalThis as { vos?: Bridge }).vos
  if (!vos) throw new Error('This page has no host: open Telemetry through vos')
  const ask = (params: Record<string, unknown>): Promise<unknown> => vos.call('vos/telemetry/query', params)
  return {
    traces: async () => (await ask({ kind: 'traces', limit: 100 })) as TraceSummary[],
    query: async (q) => (await ask({ kind: 'query', ...q })) as Found
  }
}
