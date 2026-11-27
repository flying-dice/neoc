/**
 * Pairing and agent access for the specs, the way a person or an agent would do it against the real host:
 *
 *   issuePairingCode()   runs `vos pair` with the host's own config (the code lands in the shared
 *                        system database, so the running host accepts it)
 *   pairDevice(name)     POST /pair with a fresh code and returns the session cookie
 *   callTool(name, args) an agent calls an app's tool over MCP with the token the host wrote to mcp.json
 *   callToolAt(access, name, args)  the same, at an MCP server and with a token given (an app's lease)
 */
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONFIG, DATA, HOST_BIN, ORIGIN, ROOT } from './config'

/** A fresh single-use pairing code, as the host prints it (`K7QM-3XHR`). */
export function issuePairingCode(): string {
  const run = spawnSync(HOST_BIN, ['pair', '--config', CONFIG], { cwd: ROOT, encoding: 'utf8' })
  const code = run.stdout.match(/Pairing code:\s+([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1]
  if (!code) throw new Error(`vos pair printed no code:\n${run.stdout}\n${run.stderr}`)
  return code
}

export interface PairedDevice {
  /** `vos_session`, a Playwright cookie for the host's origin. */
  cookie: { name: string; value: string; domain: string; path: string; httpOnly: boolean; sameSite: 'Strict' }
  deviceId: string
}

export async function pairDevice(deviceName: string): Promise<PairedDevice> {
  const res = await fetch(`${ORIGIN}/pair`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ code: issuePairingCode(), deviceName })
  })
  if (!res.ok) throw new Error(`POST /pair answered ${res.status}: ${await res.text()}`)
  const value = res.headers.getSetCookie().join(';').match(/vos_session=([a-f0-9]+)/)?.[1]
  if (!value) throw new Error('POST /pair set no session cookie')
  const { device } = (await res.json()) as { device: { id: string } }
  return {
    cookie: { name: 'vos_session', value, domain: '127.0.0.1', path: '/', httpOnly: true, sameSite: 'Strict' },
    deviceId: device.id
  }
}

/** Where an agent reaches an MCP server, and the header that says who it is. */
export interface McpAccess {
  url: string
  headers: { Authorization: string }
}

const mcpHeaders = (access: McpAccess, extra: Record<string, string> = {}): Record<string, string> => ({
  ...access.headers,
  'content-type': 'application/json',
  accept: 'application/json, text/event-stream',
  ...extra
})

/** One JSON-RPC result out of an MCP response (plain JSON or a server-sent event stream). */
async function resultOf(res: Response): Promise<{ result?: unknown; error?: { message: string } }> {
  const text = await res.text()
  const data = res.headers.get('content-type')?.includes('text/event-stream')
    ? (text.split('\n').find((l) => l.startsWith('data:') && l.includes('"jsonrpc"')) ?? '').slice(5)
    : text
  return JSON.parse(data || '{}')
}

/**
 * An agent calls `<app>__<tool>` over MCP (Streamable HTTP), authenticated as the host's local agent: the
 * token the host wrote to `<data>/mcp.json` at start. Returns the tool result's text.
 */
export async function callTool(tool: string, args: Record<string, unknown>): Promise<string> {
  return callToolAt(JSON.parse(readFileSync(join(DATA, 'mcp.json'), 'utf8')) as McpAccess, tool, args)
}

/** An agent calls `<app>__<tool>` over MCP (Streamable HTTP) at `access`. Returns the tool result's text. */
export async function callToolAt(access: McpAccess, tool: string, args: Record<string, unknown>): Promise<string> {
  const post = (body: object, session?: string): Promise<Response> =>
    fetch(access.url, {
      method: 'POST',
      headers: mcpHeaders(access, session ? { 'mcp-session-id': session } : {}),
      body: JSON.stringify(body)
    })
  const init = await post({
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'e2e-agent', version: '1' } }
  })
  const session = init.headers.get('mcp-session-id') ?? undefined
  await resultOf(init)
  await post({ jsonrpc: '2.0', method: 'notifications/initialized' }, session)
  const call = await resultOf(
    await post({ jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: tool, arguments: args } }, session)
  )
  if (call.error) throw new Error(call.error.message)
  const out = call.result as { isError?: boolean; content: { text: string }[] }
  const text = out.content.map((c) => c.text).join('\n')
  if (out.isError) throw new Error(text)
  return text
}
