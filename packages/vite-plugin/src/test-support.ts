/** What the tests of this package share. */
import { mkdtempSync, rmSync, symlinkSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * Whether this account may make a symbolic link. Windows refuses (`EPERM`) unless Developer Mode is on or the process is
 * elevated; a test that needs a link is skipped there, since no link can exist for the code under test to meet.
 */
export const canLink: boolean = (() => {
  const dir = mkdtempSync(join(tmpdir(), 'vos-link-probe-'))
  try {
    symlinkSync(dir, join(dir, 'link'))
    return true
  } catch {
    return false
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})()
