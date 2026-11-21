/**
 * Build one Luau entry file into its bundled Luau text, the same way `build-apps.ts` builds an app:
 *
 *   bun scripts/luau-build.ts <entry.luau> <out.luau>
 *
 * For the Rust tests, which run real apps and fixtures through the host's Luau runtime: they build with
 * the product's own pipeline (scripts/luau.ts, .darklua.json) instead of carrying generated copies.
 */
import { writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { requireDarklua, toLuau } from './luau'

const [entry, out] = process.argv.slice(2)
if (entry === undefined || out === undefined) {
  console.error('usage: bun scripts/luau-build.ts <entry.luau> <out.luau>')
  process.exit(2)
}

try {
  writeFileSync(resolve(out), toLuau(requireDarklua(), resolve(entry)))
} catch (error) {
  console.error(`darklua could not build ${entry}:\n${(error as Error).message}`)
  process.exit(1)
}
