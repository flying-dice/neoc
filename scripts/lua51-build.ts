#!/usr/bin/env bun
/**
 * Build one Lua entry file into its bundled Lua 5.1 text, the same way `build-apps.ts` builds an app:
 *
 *   bun scripts/lua51-build.ts <entry.lua> <out.lua>
 *
 * For the Rust tests, which run real apps and fixtures through the host's Lua 5.1 runtime: they build with
 * the product's own pipeline (scripts/lua51.ts, .darklua.json) instead of carrying generated copies.
 */
import { writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { requireDarklua, toLua51 } from './lua51'

const [entry, out] = process.argv.slice(2)
if (entry === undefined || out === undefined) {
  console.error('usage: bun scripts/lua51-build.ts <entry.lua> <out.lua>')
  process.exit(2)
}

try {
  writeFileSync(resolve(out), toLua51(requireDarklua(), resolve(entry)))
} catch (error) {
  console.error(`darklua could not build ${entry}:\n${(error as Error).message}`)
  process.exit(1)
}
