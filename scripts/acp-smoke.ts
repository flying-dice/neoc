#!/usr/bin/env bun
/**
 * Dev smoke test for the agent stack, without the UI:
 *   ACP client (AgentHost) → Claude ACP adapter → AgentOS MCP server → app tools
 *
 * Requires the AgentOS server running (it writes <data>/mcp.json; AGENTOS_DATA,
 * default ~/.agentos) and a Claude login. Approves every permission request once — dev use only.
 *
 *   bun scripts/acp-smoke.ts "Use the calculator to show 1337 * 42"
 */
import { readFileSync, mkdtempSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { AgentHost } from '../apps/server/src/acp/agentHost'

const discovery = JSON.parse(
  readFileSync(join(process.env.AGENTOS_DATA ?? join(homedir(), '.agentos'), 'mcp.json'), 'utf8')
) as { url: string; headers: { Authorization: string } }

const adapter = join(
  dirname(require.resolve('@agentclientprotocol/claude-agent-acp/package.json')),
  'dist/index.js'
)

const prompt =
  process.argv[2] ??
  'Using the AgentOS calculator tools, work out (1337 * 42) + 7 and show the result on the calculator for me.'

const host = new AgentHost(
  [{ id: 'claude', name: 'Claude', command: 'node', args: [adapter] }],
  { url: discovery.url, token: discovery.headers.Authorization.replace(/^Bearer /, ''), close: async () => {} },
  {
    update: (_agentId, { update }) => {
      switch (update.sessionUpdate) {
        case 'agent_message_chunk':
          if (update.content.type === 'text') process.stdout.write(update.content.text)
          break
        case 'tool_call':
          console.log(`\n[tool_call] ${update.title} (${update.status ?? 'pending'})`)
          break
        case 'tool_call_update':
          if (update.status) console.log(`[tool_update] ${update.toolCallId} → ${update.status}`)
          break
      }
    },
    permission: ({ id, params }) => {
      const allow = params.options.find((o) => o.kind === 'allow_once') ?? params.options[0]
      console.log(`\n[permission] ${params.toolCall.title} → ${allow?.name}`)
      host.respondPermission(id, allow?.optionId ?? null)
    },
    exit: (agentId, code) => console.log(`\n[exit] ${agentId} code=${code}`)
  },
  mkdtempSync(join(tmpdir(), 'agentos-smoke-'))
)

try {
  const session = await host.newSession('claude')
  console.log('[session]', session)
  const stop = await host.prompt('claude', session.sessionId, prompt)
  console.log(`\n[stop] ${stop}`)
} finally {
  host.stopAll()
}
