import { randomBytes, timingSafeEqual } from 'node:crypto'
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { WebStandardStreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/webStandardStreamableHttp.js'
import type { ToolRegistry } from '../tools/registry'

export interface McpEndpoint {
  url: string
  /** Bearer token required on every request (per server start). */
  token: string
}

/**
 * The universal AgentOS MCP endpoint (`/mcp` on the Bun server): every app
 * tool from the registry, over Streamable HTTP, guarded by a bearer token.
 * Stateless: each request gets a fresh MCP server bound to the current tool
 * list, so apps registered or unregistered at runtime show up immediately.
 */
export function createMcpHandler(tools: ToolRegistry, version: string): {
  token: string
  handle(request: Request): Promise<Response>
} {
  const token = randomBytes(24).toString('hex')
  const expected = Buffer.from(`Bearer ${token}`)

  const authorized = (request: Request): boolean => {
    const given = Buffer.from(request.headers.get('authorization') ?? '')
    return given.length === expected.length && timingSafeEqual(given, expected)
  }

  const buildServer = (): McpServer => {
    const server = new McpServer({ name: 'agentos', version })
    for (const { info, shape } of tools.entries()) {
      server.registerTool(
        info.id,
        { title: info.title, description: info.description, inputSchema: shape, annotations: { readOnlyHint: info.readOnly } },
        async (args) => {
          try {
            const result = await tools.call(info.id, args)
            return typeof result === 'string'
              ? { content: [{ type: 'text' as const, text: result }] }
              : { content: [{ type: 'text' as const, text: JSON.stringify(result) }], structuredContent: result }
          } catch (error) {
            return {
              content: [{ type: 'text' as const, text: error instanceof Error ? error.message : String(error) }],
              isError: true
            }
          }
        }
      )
    }
    return server
  }

  return {
    token,
    async handle(request) {
      if (!authorized(request)) return new Response('Unauthorized', { status: 401 })
      if (request.method !== 'POST') return new Response('Method Not Allowed', { status: 405, headers: { Allow: 'POST' } })
      const server = buildServer()
      const transport = new WebStandardStreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true })
      await server.connect(transport)
      try {
        return await transport.handleRequest(request)
      } finally {
        void transport.close()
        void server.close()
      }
    }
  }
}
