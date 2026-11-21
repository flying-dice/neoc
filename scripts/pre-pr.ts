#!/usr/bin/env bun
/**
 * `bun run gate`: the one pre-PR command set for the whole repository.
 *
 *   1. the neoc checks (`scripts/check-neoc.sh`: fmt, clippy, the neoc and neoc-vm tests, the Lua suites, the bench
 *      build, the toolchain pin, cargo deny, the Luau declarations, vos-codegen --check, the no-vos-leak dependency
 *      check, `cargo check --workspace --all-targets --locked`);
 *   2. the vos checks: `bun run lint`, `typecheck`, `test`, `test:e2e` (a Mac, for the screenshot baselines).
 *
 * It is fast on purpose (iteration speed is a requirement: CLAUDE.md, "The gate"). The slow suites are not in it: `bun run
 * slow` (`scripts/check-slow.sh`). e2e is required only for a change to vos UI, apps, the client, the protocol or the
 * host's HTTP surface; `--no-e2e` leaves it out for any other change.
 *
 * It runs with `VOS_GATE=1` and `NEOC_REQUIRE_LUAU_LSP=1` so that a missing luau-lsp, stylua or darklua fails instead of
 * skipping (a skipped check is not a pass), stops at the first failing step and prints what ran. A change confined to the
 * neoc crates can run `scripts/check-neoc.sh` alone (no bun, no darklua): `bun run gate:neoc`, see CONTRIBUTING.md.
 *
 * Usage: bun run gate [--neoc | --vos] [--no-e2e]
 *   --neoc / --vos  a half of it, for a loop; the gate before a PR is the whole of it (or --no-e2e, per CLAUDE.md)
 *   --no-e2e        everything but test:e2e
 */
import { resolve } from 'node:path'

export interface Step {
  name: string
  cmd: string[]
}

/** The environment of every step: the two "a missing tool fails" switches, whichever half asks for them. */
export const GATE_ENV = { VOS_GATE: '1', NEOC_REQUIRE_LUAU_LSP: '1' } as const

const NEOC: Step[] = [{ name: 'neoc checks', cmd: ['bash', 'scripts/check-neoc.sh'] }]
const VOS: Step[] = [
  { name: 'lint', cmd: ['bun', 'run', 'lint'] },
  { name: 'typecheck', cmd: ['bun', 'run', 'typecheck'] },
  { name: 'test', cmd: ['bun', 'run', 'test'] },
  { name: 'test:e2e', cmd: ['bun', 'run', 'test:e2e'] }
]

/** The steps to run, in order, for the flags given (none: the whole gate). */
export function plan(args: string[]): Step[] {
  const known = ['--neoc', '--vos', '--no-e2e']
  const unknown = args.filter((a) => !known.includes(a))
  if (unknown.length) throw new Error(`unknown argument ${unknown.join(' ')}: the flags are ${known.join(', ')}`)
  const neoc = args.includes('--neoc')
  const vos = args.includes('--vos')
  if (neoc && vos) throw new Error('--neoc and --vos together is the whole gate: pass neither')
  const vosSteps = args.includes('--no-e2e') ? VOS.filter((s) => s.name !== 'test:e2e') : VOS
  if (neoc) return NEOC
  if (vos) return vosSteps
  return [...NEOC, ...vosSteps]
}

async function main(): Promise<number> {
  let steps: Step[]
  try {
    steps = plan(process.argv.slice(2).filter((a) => a !== '--'))
  } catch (e) {
    console.error(String(e instanceof Error ? e.message : e))
    return 2
  }
  const root = resolve(import.meta.dir, '..')
  const done: string[] = []
  for (const step of steps) {
    const started = Date.now()
    console.log(`\n### gate: ${step.name}  (${step.cmd.join(' ')})`)
    const proc = Bun.spawn(step.cmd, { cwd: root, env: { ...process.env, ...GATE_ENV }, stdio: ['inherit', 'inherit', 'inherit'] })
    const code = await proc.exited
    const secs = Math.round((Date.now() - started) / 1000)
    if (code !== 0) {
      console.error(`\n### gate FAILED at "${step.name}" (exit ${code}, ${secs}s)`)
      if (done.length) console.error(`passed before it: ${done.join(', ')}`)
      return code
    }
    done.push(`${step.name} (${secs}s)`)
  }
  console.log(`\n### gate passed: ${done.join(', ')}`)
  return 0
}

if (import.meta.main) process.exit(await main())
