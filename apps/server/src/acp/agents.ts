import { dirname, join } from 'node:path'
import type { AgentDefinition } from './agentHost'

/** Absolute path to a bundled adapter's CLI entry, resolved from node_modules. */
function packageBin(pkg: string, bin: string): string {
  return join(dirname(require.resolve(`${pkg}/package.json`)), bin)
}

/**
 * ACP agents AgentOS can run. Any program that speaks ACP over stdio works.
 * The Claude adapter targets Node (>= 22), so it runs on `node` when it's on
 * PATH and falls back to Bun's Node compatibility otherwise. Credentials come
 * from each agent's own login (Claude: your Claude Code login or ANTHROPIC_API_KEY).
 */
export function agentDefinitions(): AgentDefinition[] {
  return [
    {
      id: 'claude',
      name: 'Claude',
      command: Bun.which('node') ?? process.execPath,
      args: [packageBin('@agentclientprotocol/claude-agent-acp', 'dist/index.js')]
    }
  ]
}
