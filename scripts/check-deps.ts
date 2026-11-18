#!/usr/bin/env bun
/**
 * Dependency-direction gate for the Rust host's hexagonal layout (docs/architecture/ARCHITECTURE.md §2).
 *
 * Reads `cargo metadata` and checks every workspace crate's NORMAL (non-dev, non-build) dependencies
 * against an explicit allow-list:
 *
 *   domain      -> nothing internal; external: serde, serde_json, sha2, thiserror (+ ts-rs, schemars)
 *   application -> domain; external: async-trait, futures-channel, parking_lot, serde_json, sha2, thiserror, tracing
 *   adapters    -> application, domain; never each other
 *   lua         -> application, domain, luavm (the one adapter that may use the Lua VM library)
 *   luavm       -> nothing internal (the Lua VM library; only the lua adapter may depend on it)
 *   host        -> everything
 *
 * A crate that is not in INTERNAL fails the gate, so a new crate has to be placed in the hexagon on purpose.
 * `cargo deny` (deny.toml `bans` with `wrappers`) is the second line of defence for the framework crates.
 *
 * Why a bun script and not a Rust xtask: it runs with the other lint scripts (`bun run lint`), needs no
 * compile step, and still checks the layout when the Rust build itself is broken.
 *
 * Usage: bun scripts/check-deps.ts   (exit 1 on a violation)
 */
import { spawnSync } from 'node:child_process'

const INTERNAL: Record<string, string[]> = {
  'agentos-domain': [],
  'agentos-application': ['agentos-domain'],
  // The Lua adapter runs apps (plain Lua 5.1) on top of the Lua VM library below.
  'agentos-lua': ['agentos-domain', 'agentos-application', 'agentos-luavm'],
  // The Lua 5.1 VM: a support library, not an adapter. It depends on no agentos crate, and only the Lua
  // adapter may depend on it (the one place a script engine is wired to a port), so the VM stays swappable.
  'agentos-luavm': [],
  'agentos-sqlite': ['agentos-domain', 'agentos-application'],
  'agentos-fetch': ['agentos-application'],
  'agentos-mcp': ['agentos-domain', 'agentos-application'],
  'agentos-acp': ['agentos-domain', 'agentos-application'],
  'agentos-agui': ['agentos-domain', 'agentos-application'],
  'agentos-http': ['agentos-domain', 'agentos-application'],
  'agentos-codegen': ['agentos-domain'],
  'agentos-host': [
    'agentos-domain',
    'agentos-application',
    'agentos-lua',
    'agentos-sqlite',
    'agentos-fetch',
    'agentos-mcp',
    'agentos-acp',
    'agentos-agui',
    'agentos-http'
  ]
}

/** Crates whose EXTERNAL dependencies are restricted too (the pure core). */
const CORE_EXTERNAL: Record<string, Set<string>> = {
  'agentos-domain': new Set(['serde', 'serde_json', 'sha2', 'thiserror', 'ts-rs', 'schemars']),
  'agentos-application': new Set(['async-trait', 'futures-channel', 'parking_lot', 'serde_json', 'sha2', 'thiserror', 'tracing'])
}

/** Framework and runtime crates that never appear in the core, whatever the tables above say. */
const FRAMEWORKS = new Set([
  'tokio',
  'axum',
  'axum-server',
  'mlua',
  'rusqlite',
  'reqwest',
  'rmcp',
  'agent-client-protocol',
  'tower-http',
  'notify',
  'getrandom'
])

interface Dependency {
  name: string
  kind: string | null
}
interface Metadata {
  packages: { name: string; dependencies: Dependency[] }[]
}

const run = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024
})
if (run.status !== 0) {
  console.error(`check-deps: \`cargo metadata\` failed\n${run.stderr}`)
  process.exit(1)
}
const metadata = JSON.parse(run.stdout) as Metadata

const problems: string[] = []
const seen = new Set<string>()
for (const pkg of metadata.packages) {
  const allowed = INTERNAL[pkg.name]
  if (!allowed) {
    problems.push(`${pkg.name}: workspace crate is not placed in the hexagon (add it to INTERNAL in scripts/check-deps.ts)`)
    continue
  }
  seen.add(pkg.name)
  const normal = pkg.dependencies.filter((d) => d.kind === null)
  for (const { name } of normal) {
    const internal = name.startsWith('agentos-')
    if (internal && !allowed.includes(name)) {
      problems.push(`${pkg.name} -> ${name}: internal edge not allowed (allowed: ${allowed.join(', ') || 'none'})`)
    }
    const core = CORE_EXTERNAL[pkg.name]
    if (core && !internal && !core.has(name)) {
      problems.push(`${pkg.name} -> ${name}: external crate is not on the core allow-list`)
    }
    if (core && FRAMEWORKS.has(name)) {
      problems.push(`${pkg.name} -> ${name}: framework or runtime crate in the core`)
    }
  }
}
for (const name of Object.keys(INTERNAL)) {
  if (!seen.has(name)) problems.push(`${name}: listed in INTERNAL but not in the workspace`)
}

if (problems.length > 0) {
  console.error(`dependency direction violations:\n  ${problems.join('\n  ')}`)
  process.exit(1)
}
console.log(`dependency direction OK (${seen.size} crates checked)`)
