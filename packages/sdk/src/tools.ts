import type { z } from 'zod'

/**
 * A tool an app exposes to the universal AgentOS tool registry. The same tool
 * is callable by ACP agents (through the host's MCP server) and by any app
 * renderer (through `api.tools.call`).
 */
export interface AppToolDefinition<Shape extends z.ZodRawShape = z.ZodRawShape> {
  /** Local name, unique within the app: letters, digits, `_` and `-`. */
  name: string
  title?: string
  /** What the tool does and when to use it — agents read this to decide. */
  description: string
  /** Zod object shape describing the input. */
  input: Shape
  /** True when the tool only reads state (lets agents call it without approval). */
  readOnly?: boolean
  run: (args: z.infer<z.ZodObject<Shape>>) => ToolResult | Promise<ToolResult>
}

/** Plain text, or a JSON object returned as structured content. */
export type ToolResult = string | Record<string, unknown>

/** Tool metadata as seen by callers (no implementation). */
export interface ToolInfo {
  /** Globally unique name: `<appId>__<name>`. */
  id: string
  appId: string
  name: string
  title?: string
  description: string
  readOnly: boolean
}

export const qualifyTool = (appId: string, name: string): string =>
  `${appId.replace(/-/g, '_')}__${name}`
