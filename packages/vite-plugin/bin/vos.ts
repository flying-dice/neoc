#!/usr/bin/env bun
/**
 * `vos`: what an app project's scripts call.
 *
 *   vos generate       write .app/ (the typed client, and the router and types of the backend's language) from openrpc.json
 *   vos typecheck      generate, then check the backend: luau-lsp over the Luau (strict, against the host's declarations),
 *                      or the TypeScript compiler over a backend in TypeScript
 *   vos lint           the same, and StyLua is satisfied
 *   vos format         StyLua over the Luau
 *   vos bundle <out>   generate, then write the backend as the build bundles it (the app's `app.luau`) to <out>
 */
import { writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { backendBundle } from '../src/backend.ts'
import { DefinitionError } from '../src/definition.ts'
import { generate, isTypeScript, removeOtherBackend, resolveOptions, write } from '../src/generate.ts'
import { analyze, stylua } from '../src/luau-tools.ts'

const [command, argument] = process.argv.slice(2)
const project = resolve(process.cwd())
const settings = resolveOptions(project)

function generated(): void {
  const written = write(project, generate(project, settings))
  removeOtherBackend(project, settings)
  if (command === 'generate') console.log(written.length > 0 ? `wrote ${written.join(', ')}` : 'the generated files are up to date')
}

function typecheck(): boolean {
  if (isTypeScript(settings)) {
    // compiling is the check: the types, and what TypeScriptToLua does not compile (the error names every problem)
    backendBundle(project, settings)
    console.log(`typescript check OK (${settings.backend}, compiled to Luau)`)
    return true
  }
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
    case 'generate':
      generated()
      break
    case 'typecheck':
      generated()
      process.exit(typecheck() ? 0 : 1)
      break
    case 'lint':
      generated()
      process.exit([typecheck(), stylua(project, false)].every(Boolean) ? 0 : 1)
      break
    case 'format':
      process.exit(stylua(project, true) ? 0 : 1)
      break
    case 'bundle':
      if (argument === undefined) {
        console.error('usage: vos bundle <out.luau>')
        process.exit(2)
      }
      generated()
      writeFileSync(resolve(argument), backendBundle(project, settings))
      break
    default:
      console.error('usage: vos generate | typecheck | lint | format | bundle <out.luau>')
      process.exit(2)
  }
} catch (e) {
  console.error(e instanceof DefinitionError ? e.message : (e as Error).message)
  process.exit(1)
}
