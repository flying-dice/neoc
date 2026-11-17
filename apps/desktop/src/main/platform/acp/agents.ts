import { dirname, join } from 'node:path'
import type { AgentDefinition } from './agentHost'

/** Absolute path to a bundled adapter's CLI entry, resolved from node_modules. */
function packageBin(pkg: string, bin: string): string {
  return join(dirname(require.resolve(`${pkg}/package.json`)), bin)
}

/**
 * ACP agents AgentOS can run. Any program that speaks ACP over stdio works.
 * Bundled adapters run on Electron's own Node (ELECTRON_RUN_AS_NODE), so no
 * system Node or PATH setup is needed. Credentials come from each agent's own
 * login (Claude uses your Claude Code login or ANTHROPIC_API_KEY).
 */
export function agentDefinitions(): AgentDefinition[] {
  return [
    {
      id: 'claude',
      name: 'Claude',
      command: process.execPath,
      args: [packageBin('@agentclientprotocol/claude-agent-acp', 'dist/index.js')],
      env: { ELECTRON_RUN_AS_NODE: '1' }
    }
  ]
}
