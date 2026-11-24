#!/usr/bin/env bun
/**
 * Dependency-direction gate for the Rust host's hexagonal layout.
 *
 * Reads `cargo metadata` and checks every workspace crate's normal dependencies against an explicit allow-list
 * (and, for the framework crates in FRAMEWORK_HOMES, its build dependencies too; a dev-dependency is exempt):
 *
 *   domain      -> nothing internal; external: serde, serde_json, sha2, thiserror (+ ts-rs)
 *   application -> domain; external: async-trait, futures-util (its runtime-free `FutureExt::catch_unwind`), parking_lot, serde_json, sha2, thiserror, tracing
 *   adapters    -> application, domain; never each other (but codegen, below)
 *   codegen     -> domain, lua (dev-time: it writes the declarations from the schema of the modules the Lua adapter links)
 *   lua         -> application, domain, neoc-vm2 (the one vos crate that may use the Lua VM library)
 *   neoc-vm2    -> nothing internal (the Lua VM library, on mlua-ir; the lua adapter and the `neoc` CLI may depend on it)
 *   neoc        -> neoc-vm2 only (the CLI over the VM; it never reaches into the vos crates)
 *   host        -> every adapter, never the VM
 *
 * The workspace holds two products, the vos hexagon and neoc (`neoc-vm2` and the `neoc` CLI); the lists below
 * keep each side's edges inside its own. A crate that is not in INTERNAL fails the gate, so a new crate has to be
 * placed on purpose.
 * `cargo deny` (deny.toml `bans` with `wrappers`) is the second line of defence for the framework crates.
 *
 * Why a bun script and not a Rust xtask: it runs with the other lint scripts (`bun run lint`), needs no
 * compile step, and still checks the layout when the Rust build itself is broken.
 *
 * Usage: bun scripts/check-deps.ts   (exit 1 on a violation)
 */
import { spawnSync } from 'node:child_process'

const INTERNAL: Record<string, string[]> = {
  'vos-domain': [],
  'vos-application': ['vos-domain'],
  // The Lua adapter runs apps (Luau source) on top of the Lua VM library below.
  'vos-lua': ['vos-domain', 'vos-application', 'neoc-vm2'],
  // The Lua VM (Luau): a support library, not an adapter. It depends on no crate here, and among the vos crates only
  // the Lua adapter may depend on it (the one place a script engine is wired to a port), so the VM stays swappable.
  // The `neoc` CLI is its other client.
  'neoc-vm2': [],
  neoc: ['neoc-vm2'],
  'vos-sqlite': ['vos-domain', 'vos-application'],
  'vos-fetch': ['vos-application'],
  'vos-mcp': ['vos-domain', 'vos-application'],
  'vos-http': ['vos-domain', 'vos-application'],
  // The dev-time generator reads the schema of what a script can reach from the Lua adapter, which links the modules
  // (the `app` API and the VM library's): the one adapter that depends on another, and it is never linked into the host.
  'vos-codegen': ['vos-domain', 'vos-lua'],
  'vos-host': [
    'vos-domain',
    'vos-application',
    'vos-lua',
    'vos-sqlite',
    'vos-fetch',
    'vos-mcp',
    'vos-http'
  ]
}

/** Crates whose EXTERNAL dependencies are restricted too (the pure core). */
const CORE_EXTERNAL: Record<string, Set<string>> = {
  'vos-domain': new Set(['serde', 'serde_json', 'sha2', 'thiserror', 'ts-rs']),
  'vos-application': new Set(['async-trait', 'futures-util', 'parking_lot', 'serde_json', 'sha2', 'thiserror', 'tracing'])
}

/**
 * Where each framework crate may be a normal or build dependency. deny.toml `bans` pins the same kind of crates, but its
 * `wrappers` also match dev-dependencies, so that file alone would let one of these move from a test dependency to a
 * runtime one. The two lists differ on purpose: deny.toml also bans the VM crates, `openssl` and `native-tls`, and this table
 * names `tonic`, which deny.toml lets use axum but no workspace crate may depend on.
 */
const FRAMEWORK_HOMES: Record<string, string[]> = {
  mlua: ['neoc-vm2', 'neoc'],
  rusqlite: ['vos-sqlite', 'neoc-vm2'],
  rmcp: ['vos-mcp'],
  axum: ['vos-http', 'vos-mcp', 'vos-host'],
  'axum-server': ['vos-host'],
  // reached only through the VM's dependencies (deny.toml lets tonic use axum); no workspace crate names it
  tonic: []
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
  'tower-http',
  'notify',
  'getrandom'
])

interface Dependency {
  name: string
  kind: string | null
}
export interface WorkspaceCrate {
  name: string
  dependencies: Dependency[]
}

/** What `cargo metadata --no-deps` says about the workspace crates, checked against the tables above. */
export function findProblems(packages: WorkspaceCrate[]): { problems: string[]; seen: Set<string> } {
  const problems: string[] = []
  const seen = new Set<string>()
  for (const pkg of packages) {
    const allowed = INTERNAL[pkg.name]
    if (!allowed) {
      problems.push(`${pkg.name}: workspace crate is not placed (add it to INTERNAL in scripts/check-deps.ts)`)
      continue
    }
    seen.add(pkg.name)
    // a framework crate is held to its home whether it is a normal or a build dependency; a dev-dependency (a test
    // client or server) is exempt
    for (const { name, kind } of pkg.dependencies) {
      const homes = FRAMEWORK_HOMES[name]
      if (kind !== 'dev' && homes && !homes.includes(pkg.name)) {
        problems.push(
          `${pkg.name} -> ${name}: framework crate is not allowed here as a ${kind ?? 'normal'} dependency (allowed in: ${homes.join(', ') || 'none'})`
        )
      }
    }
    for (const { name } of pkg.dependencies.filter((d) => d.kind === null)) {
      const internal = name in INTERNAL
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
  return { problems, seen }
}

if (import.meta.main) {
  const run = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024
  })
  if (run.status !== 0) {
    console.error(`check-deps: \`cargo metadata\` failed\n${run.stderr || run.error}`)
    process.exit(1)
  }
  const { problems, seen } = findProblems((JSON.parse(run.stdout) as { packages: WorkspaceCrate[] }).packages)
  if (problems.length > 0) {
    console.error(`dependency direction violations:\n  ${problems.join('\n  ')}`)
    process.exit(1)
  }
  console.log(`dependency direction OK (${seen.size} crates checked)`)
}
