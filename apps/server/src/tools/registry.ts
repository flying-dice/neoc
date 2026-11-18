import { z } from 'zod'
import { qualifyTool, type AppToolDefinition, type ToolInfo, type ToolResult } from '@agentos/sdk'

interface RegisteredTool {
  info: ToolInfo
  shape: z.ZodRawShape
  run: (args: never) => ToolResult | Promise<ToolResult>
}

/**
 * The universal tool registry. Apps contribute tools through the SDK; the
 * registry serves them to ACP agents (via the MCP server) and to app
 * renderers (via IPC), validating every call against the tool's schema.
 */
export class ToolRegistry {
  #tools = new Map<string, RegisteredTool>()

  register<Shape extends z.ZodRawShape>(appId: string, def: AppToolDefinition<Shape>): void {
    if (!/^[A-Za-z0-9_-]{1,40}$/.test(def.name)) {
      throw new Error(`[${appId}] invalid tool name "${def.name}"`)
    }
    const id = qualifyTool(appId, def.name)
    if (this.#tools.has(id)) throw new Error(`Tool "${id}" is already registered`)
    this.#tools.set(id, {
      info: {
        id,
        appId,
        name: def.name,
        title: def.title,
        description: def.description,
        readOnly: def.readOnly ?? false
      },
      shape: def.input,
      run: def.run as RegisteredTool['run']
    })
  }

  /** Remove every tool an app registered (the app is being unregistered). */
  unregisterApp(appId: string): void {
    for (const [id, tool] of this.#tools) if (tool.info.appId === appId) this.#tools.delete(id)
  }

  list(): ToolInfo[] {
    return [...this.#tools.values()].map((t) => t.info)
  }

  /** Every registered tool with its input schema (for the MCP server). */
  entries(): { info: ToolInfo; shape: z.ZodRawShape }[] {
    return [...this.#tools.values()].map(({ info, shape }) => ({ info, shape }))
  }

  async call(id: string, args: unknown): Promise<ToolResult> {
    const tool = this.#tools.get(id)
    if (!tool) throw new Error(`Unknown tool "${id}"`)
    const parsed = z.object(tool.shape).parse(args ?? {})
    return tool.run(parsed as never)
  }
}
