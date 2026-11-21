/**
 * What a check does when its tool is not installed (`lint:luau` needs luau-lsp, `lint:stylua` needs
 * stylua).
 *
 * On a developer machine it SKIPS with a message and exits 0, so a fresh checkout still lints what it can. In the
 * quality gate it FAILS: a skipped check is not a pass. The gate is a run with `VOS_GATE=1` (the `vos-pre-commit`
 * skill sets it) or with `CI` set (the usual flag of an automated run).
 */

/** Whether this run is the quality gate: `VOS_GATE=1`, or a `CI` that is set and not `false`/`0`. */
export function isGateRun(env: Record<string, string | undefined> = process.env): boolean {
  if (env.VOS_GATE === '1') return true
  const ci = env.CI
  return ci !== undefined && ci !== '' && ci !== 'false' && ci !== '0'
}

/** What to do about a missing tool: print how it ended and the exit code (0 for a skip, 1 in the gate). */
export function missingTool(
  check: string,
  message: string,
  env: Record<string, string | undefined> = process.env
): { line: string; code: number } {
  return isGateRun(env)
    ? { line: `${check} FAILED: ${message}\n  (VOS_GATE or CI is set: a skipped check is not a pass)`, code: 1 }
    : { line: `${check} SKIPPED: ${message}`, code: 0 }
}

/** Ends the process for a missing tool, as [`missingTool`] says. */
export function skipTool(check: string, message: string): never {
  const { line, code } = missingTool(check, message)
  if (code === 0) console.log(line)
  else console.error(line)
  process.exit(code)
}
