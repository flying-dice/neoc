import { createServer, type IncomingMessage, type Server } from 'node:http'
import { randomBytes, timingSafeEqual } from 'node:crypto'
import type { AddressInfo } from 'node:net'
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'
import type { ToolRegistry } from './toolRegistry'

export interface McpEndpoint {
  url: string
  /** Bearer token required on every request. */
  token: string
  close(): Promise<void>
}

/**
 * The universal AgentOS MCP server: exposes every app tool from the
 * registry over Streamable HTTP on 127.0.0.1, guarded by a per-launch bearer
 * token. Stateless — each request gets a fresh server bound to the current
 * tool list, so newly installed tools appear without reconnecting.
 */
export async function startMcpServer(tools: ToolRegistry, version: string): Promise<McpEndpoint> {
  const token = randomBytes(24).toString('hex')

  const buildServer = (): McpServer => {
    const server = new McpServer({ name: 'agentos', version })
    for (const { info, shape } of tools.entries()) {
      server.registerTool(
        info.id,
        {
          title: info.title,
          description: info.description,
          inputSchema: shape,
          annotations: { readOnlyHint: info.readOnly }
        },
        async (args) => {
          try {
            const result = await tools.call(info.id, args)
            return typeof result === 'string'
              ? { content: [{ type: 'text' as const, text: result }] }
              : {
                  content: [{ type: 'text' as const, text: JSON.stringify(result) }],
                  structuredContent: result
                }
          } catch (error) {
            return {
              content: [
                {
                  type: 'text' as const,
                  text: error instanceof Error ? error.message : String(error)
                }
              ],
              isError: true
            }
          }
        }
      )
    }
    return server
  }

  const authorized = (req: IncomingMessage): boolean => {
    const header = req.headers.authorization ?? ''
    const expected = Buffer.from(`Bearer ${token}`)
    const given = Buffer.from(header)
    return given.length === expected.length && timingSafeEqual(given, expected)
  }

  const http: Server = createServer(async (req, res) => {
    if (req.url !== '/mcp') return void res.writeHead(404).end()
    if (!authorized(req)) return void res.writeHead(401).end()
    if (req.method !== 'POST') {
      // Stateless server: no standalone SSE stream or session deletion.
      return void res.writeHead(405, { Allow: 'POST' }).end()
    }
    try {
      const body = await readJson(req)
      const server = buildServer()
      const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined })
      res.on('close', () => {
        void transport.close()
        void server.close()
      })
      await server.connect(transport)
      await transport.handleRequest(req, res, body)
    } catch (error) {
      console.error('[agentos] MCP request failed:', error)
      if (!res.headersSent) res.writeHead(500).end()
    }
  })

  await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve))
  const { port } = http.address() as AddressInfo
  return {
    url: `http://127.0.0.1:${port}/mcp`,
    token,
    close: () => new Promise((resolve) => http.close(() => resolve()))
  }
}

function readJson(req: IncomingMessage): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    let size = 0
    req.on('data', (chunk: Buffer) => {
      size += chunk.length
      if (size > 4 * 1024 * 1024) {
        reject(new Error('Request too large'))
        req.destroy()
      } else chunks.push(chunk)
    })
    req.on('end', () => {
      try {
        resolve(chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : undefined)
      } catch (error) {
        reject(error)
      }
    })
    req.on('error', reject)
  })
}
