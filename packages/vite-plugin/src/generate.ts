/**
 * Generation: the contract in, the typed client and the Luau router out, into the project's `.app/` folder. The output
 * is not committed (`.app/` is ignored): every script of the project (`build`, `typecheck`, `lint`) generates first, and
 * `bun run generate` does it on its own for the editor.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join } from 'node:path'
import { parseDefinition, type Definition } from './definition.ts'
import { emitDeclarations, emitModule } from './emit-client.ts'
import { emitLuau, emitLuauEntry } from './emit-luau.ts'
import { frameworksOf, kitTypes, type Manifest } from './kit-types.ts'

/**
 * The kit's Custom Elements Manifest, committed in `packages/kit` (the plugin depends on `@vos/kit` for it alone: an app
 * never bundles the kit). Read once per process.
 */
let kitManifest: Manifest | undefined
function readKitManifest(): Manifest {
  kitManifest ??= JSON.parse(readFileSync(createRequire(import.meta.url).resolve('@vos/kit/custom-elements.json'), 'utf8')) as Manifest
  return kitManifest
}

/**
 * The kit's editor types for the frameworks the project depends on (`kit-svelte.d.ts`, `kit-react.d.ts`): the project's
 * tsconfig includes `.app/*.d.ts`. A project with neither gets none.
 */
export function kitDeclarations(project: string, dir: string): GeneratedFile[] {
  const pkg = join(project, 'package.json')
  const frameworks = existsSync(pkg) ? frameworksOf(JSON.parse(readFileSync(pkg, 'utf8')) as Parameters<typeof frameworksOf>[0]) : []
  return frameworks.map((framework) => ({ path: `${dir}/kit-${framework}.d.ts`, contents: kitTypes(readKitManifest(), framework) }))
}

export interface Options {
  /** The contract, relative to the project (default `openrpc.json`). */
  definition: string
  /** Where the generated files go, relative to the project (default `.app`). */
  generated: string
  /** The backend's module: returns the handlers (default `server/main.luau`). */
  backend: string
  /** Run the Luau type check of the backend against the contract as part of the build (default true). */
  check: boolean
}

export const DEFAULTS: Options = {
  definition: 'openrpc.json',
  generated: '.app',
  backend: 'server/main.luau',
  check: true
}

export interface GeneratedFile {
  /** Relative to the project. */
  path: string
  contents: string
}

export function readDefinition(project: string, options: Options): Definition {
  const file = join(project, options.definition)
  if (!existsSync(file)) throw new Error(`${options.definition} not found in ${project}: an app defines its RPC there (OpenRPC)`)
  return parseDefinition(readFileSync(file, 'utf8'), options.definition)
}

/** The files the contract generates, and the kit's editor types for the project's framework. */
export function generate(project: string, options: Options): GeneratedFile[] {
  const definition = readDefinition(project, options)
  const dir = options.generated
  return [
    { path: `${dir}/rpc.d.ts`, contents: emitDeclarations(definition, options.definition) },
    { path: `${dir}/rpc.js`, contents: emitModule(definition, options.definition) },
    { path: `${dir}/rpc.luau`, contents: emitLuau(definition, options.definition) },
    { path: `${dir}/entry.luau`, contents: emitLuauEntry() },
    ...kitDeclarations(project, dir)
  ]
}

/** Write what changed (a file is never rewritten with the same text, so a watcher is not woken for nothing). Returns the paths written. */
export function write(project: string, files: GeneratedFile[]): string[] {
  const written: string[] = []
  for (const file of files) {
    const target = join(project, file.path)
    if (existsSync(target) && readFileSync(target, 'utf8') === file.contents) continue
    mkdirSync(join(target, '..'), { recursive: true })
    writeFileSync(target, file.contents)
    written.push(file.path)
  }
  return written
}
