#!/usr/bin/env bun
/**
 * `vos`: what an app project's scripts call.
 *
 *   vos generate          write generated/rpc.ts and generated/rpc.luau from the definition
 *   vos generate --check  fail when they are stale (part of `lint`)
 *   vos typecheck         the generated files are current, and luau-lsp finds nothing in the Luau (strict, against the host's declarations)
 *   vos lint              the same, and StyLua is satisfied
 *   vos format            StyLua over the Luau
 */
import { resolve } from 'node:path'
import { DefinitionError } from '../src/definition.ts'
import { DEFAULTS, generate, stale, write } from '../src/generate.ts'
import { analyze, stylua } from '../src/luau-tools.ts'

const [command, ...flags] = process.argv.slice(2)
const project = resolve(process.cwd())

function current(): boolean {
  const files = generate(project, DEFAULTS)
  const out = stale(project, files)
  if (out.length > 0) {
    console.error(`generated files are stale: ${out.join(', ')}\nrun \`bun run generate\` and commit the result`)
    return false
  }
  return true
}

function typecheck(): boolean {
  const { files, findings } = analyze(project)
  if (findings.length > 0) {
    console.error(`luau-lsp found ${findings.length} problem(s) in ${files.length} file(s):\n${findings.join('\n')}`)
    return false
  }
  console.log(`luau check OK (${files.length} files, strict)`)
  return true
}

try {
  switch (command) {
    case 'generate': {
      if (flags.includes('--check')) process.exit(current() ? 0 : 1)
      const written = write(project, generate(project, DEFAULTS))
      console.log(written.length > 0 ? `wrote ${written.join(', ')}` : 'generated files are up to date')
      break
    }
    case 'typecheck':
      process.exit(current() && typecheck() ? 0 : 1)
      break
    case 'lint':
      process.exit([current(), typecheck(), stylua(project, false)].every(Boolean) ? 0 : 1)
      break
    case 'format':
      process.exit(stylua(project, true) ? 0 : 1)
      break
    default:
      console.error('usage: vos generate [--check] | typecheck | lint | format')
      process.exit(2)
  }
} catch (e) {
  console.error(e instanceof DefinitionError ? e.message : (e as Error).message)
  process.exit(1)
}
