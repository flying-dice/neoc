/**
 * The vos Vite plugin. An app is one Vite project:
 *
 *   - `package.json` says what the app is: its id (the package name without its scope), version, description, and under
 *     `vos` its title, icon, tint and window (`app.ts`);
 *   - `openrpc.json` (OpenRPC 1.4) types everything the backend serves and sends (`contract.ts`);
 *   - `server/main.luau`, or `server/main.ts` (exactly one), is the backend: it serves and sends on the bus through the
 *     global `RPC` the host gives it (`RPC.on_request`, `RPC.send_notification`, ...): nothing to require;
 *   - the page is a normal Vite page whose API is the globals `RPC` and `vos` (the host's bridge defines them): nothing
 *     to import.
 *
 *     // vite.config.ts
 *     import { vos } from '@vos/vite-plugin'
 *     export default defineConfig({ plugins: [svelte(), vos()] })
 *
 * Another app it talks to is a dependency, by where it is (`vos({ dependencies: ['../app1'] })`): a folder holding that
 * app's package.json and openrpc.json. The types then cover that app's public requests and notifications, by its id.
 *
 * The plugin writes `types/` (types only: `vos.d.ts` and `<id>.d.ts` for the page, `vos.d.luau` for a Luau backend,
 * `vos.backend.d.ts` and `tsconfig.backend.json` for a TypeScript one) whenever Vite resolves its config, so every Vite
 * command leaves it current; puts the host's tags first in the page; and on `vite build` bundles the backend into
 * `dist/app.luau` (a TypeScript one is type checked and compiled to Luau by TypeScriptToLua, `typescript-backend.ts`) and
 * writes `dist/app-ref.json`, the app ref the host installs. Checking, linting and formatting Luau are the Luau tools'
 * (luau-lsp, StyLua), not the plugin's.
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import type { HtmlTagDescriptor, Plugin } from 'vite'
import { parseApp, type AppMeta } from './app.ts'
import { bundleLuau, writeApp } from './build.ts'
import { parseContract } from './contract.ts'
import { emitAppTypes, emitVosTypes, type Dependency } from './emit-ts.ts'
import { frameworksOf, kitTypes, type Manifest } from './kit-types.ts'
import { createRequire } from 'node:module'
import { emitLuauDefinitions } from './emit-luau.ts'
import { emitBackendTsconfig, emitBackendTypes } from './emit-backend-ts.ts'
import { BACKEND_DECLARATIONS, BACKEND_TS, BACKEND_TSCONFIG, compileBackend, posix, typeRoot } from './typescript-backend.ts'

export { parseApp, AppError, type AppMeta } from './app.ts'
export { parseContract, ContractError, type Contract } from './contract.ts'
export type { Dependency } from './emit-ts.ts'

export const CONTRACT = 'openrpc.json'
export const BACKEND_LUAU = 'server/main.luau'
export { BACKEND_TS } from './typescript-backend.ts'
/** Where the build writes its types (and nothing else): `types/`, beside the app's sources. */
export const GENERATED = 'types'
/** Where the built page goes, under the build's output folder: the app ref lists it as the app's static content. */
export const STATIC_DIR = 'static'

/** The host serves one copy of each to every app's page: the kit's stylesheet and script, and the bridge. */
const HOST_TAGS: HtmlTagDescriptor[] = [
  { tag: 'link', attrs: { rel: 'stylesheet', href: '/kit/tokens.css', crossorigin: '' }, injectTo: 'head-prepend' },
  { tag: 'script', attrs: { src: '/kit/kit.js' }, injectTo: 'head-prepend' },
  { tag: 'script', attrs: { src: '/apps/bridge.js' }, injectTo: 'head-prepend' }
]

export interface Options {
  /** The apps this one talks to, by where they are: paths relative to the project, each a folder holding an app. */
  dependencies?: string[]
}

/** An app's id and contract, from the folder that holds it. */
function readApp(folder: string, label: string): { app: AppMeta; contract: Dependency['contract'] } {
  for (const file of ['package.json', CONTRACT]) {
    if (!existsSync(join(folder, file))) throw new Error(`${label}: ${file} not found in ${folder} (an app is a folder with package.json and ${CONTRACT})`)
  }
  return {
    app: parseApp(readFileSync(join(folder, 'package.json'), 'utf8'), `${label}/package.json`),
    contract: parseContract(readFileSync(join(folder, CONTRACT), 'utf8'), `${label}/${CONTRACT}`)
  }
}

/** The kit's Custom Elements Manifest, read once: what the editor types of its tags are made from. */
let kitManifest: Manifest | undefined
function readKitManifest(): Manifest {
  kitManifest ??= JSON.parse(readFileSync(createRequire(import.meta.url).resolve('@vos/kit/custom-elements.json'), 'utf8')) as Manifest
  return kitManifest
}

/** The host's Luau declarations: `VOS_DEFINITIONS`, else `types/generated/vos.d.luau` of the repository the project is in. */
export function findHostDefinitions(project: string): string {
  const fromEnv = process.env.VOS_DEFINITIONS
  if (fromEnv) return resolve(fromEnv)
  for (let dir = resolve(project); ; dir = dirname(dir)) {
    const candidate = join(dir, 'types/generated/vos.d.luau')
    if (existsSync(candidate)) return candidate
    if (dirname(dir) === dir) break
  }
  throw new Error("types/generated/vos.d.luau not found above the project: set VOS_DEFINITIONS to the host's declarations")
}

/** The host's TypeScript declarations: next to the Luau ones. */
export function findHostTypeScript(project: string): string {
  const file = join(dirname(findHostDefinitions(project)), 'vos.d.ts')
  if (!existsSync(file)) throw new Error(`${file} not found: the host's TypeScript declarations are generated next to its Luau ones (bun run codegen)`)
  return file
}

/** The app's backend: `server/main.luau` or `server/main.ts`, one of them. */
export function backendOf(project: string): { language: 'luau' | 'typescript'; path: string } {
  const luau = existsSync(resolve(project, BACKEND_LUAU))
  const typescript = existsSync(resolve(project, BACKEND_TS))
  if (luau && typescript) throw new Error(`${BACKEND_LUAU} and ${BACKEND_TS} are both there: the backend is one of them`)
  if (!luau && !typescript) throw new Error(`${BACKEND_LUAU} (or ${BACKEND_TS}, for a backend in TypeScript) not found: it is the app's backend`)
  return luau ? { language: 'luau', path: BACKEND_LUAU } : { language: 'typescript', path: BACKEND_TS }
}

/** The files of a backend's types: for Luau, `vos.d.luau`; for TypeScript, its declarations and the config that checks it. */
function backendTypes(project: string, app: AppMeta, contract: Dependency['contract'], dependencies: Dependency[]): { path: string; contents: string }[] {
  if (backendOf(project).language === 'luau') {
    // the host's declarations, with the global `RPC` typed for this app and its dependencies
    return [{ path: `${GENERATED}/vos.d.luau`, contents: emitLuauDefinitions(readFileSync(findHostDefinitions(project), 'utf8'), app.id, contract, dependencies) }]
  }
  return [
    { path: `${GENERATED}/${BACKEND_DECLARATIONS}`, contents: emitBackendTypes(readFileSync(findHostTypeScript(project), 'utf8'), app.id, contract, dependencies) },
    { path: `${GENERATED}/${BACKEND_TSCONFIG}`, contents: emitBackendTsconfig(typeRoot(), posix(relative(join(project, GENERATED), project))) }
  ]
}

/** The kit's editor types (`types/kit-<framework>.d.ts`) for the frameworks the project's package.json depends on. */
function kitFiles(project: string): { path: string; contents: string }[] {
  return frameworksOf(JSON.parse(readFileSync(join(project, 'package.json'), 'utf8')) as Parameters<typeof frameworksOf>[0]).map((framework) => ({
    path: `${GENERATED}/kit-${framework}.d.ts`,
    contents: kitTypes(readKitManifest(), framework)
  }))
}

/**
 * Write the kit's editor types into `types/` of a project that is not an app (a package of kit-based components, such as
 * `@vos/docker`): only `kit-<framework>.d.ts`, for the frameworks its package.json depends on. An app gets them with the
 * rest of its types from any Vite command.
 */
export function writeKitTypes(project: string): void {
  write(resolve(project), kitFiles(resolve(project)))
}

/** The files `types/` holds, from the project's package.json and contract and those of its dependencies. */
export function generate(project: string, options: Options = {}): { app: AppMeta; files: { path: string; contents: string }[] } {
  const { app, contract } = readApp(project, '.')
  const dependencies: Dependency[] = []
  for (const path of options.dependencies ?? []) {
    const dependency = readApp(resolve(project, path), path)
    const id = dependency.app.id
    if (id === app.id) throw new Error(`dependency ${path} is this app (${id})`)
    if (dependencies.some((d) => d.id === id)) throw new Error(`dependency ${path} is ${id}, which another dependency is too`)
    dependencies.push({ id, contract: dependency.contract })
  }
  return {
    app,
    files: [
      // types only, for the page's globals `RPC` and `vos`: nothing here runs
      { path: `${GENERATED}/vos.d.ts`, contents: emitVosTypes() },
      { path: `${GENERATED}/${app.id}.d.ts`, contents: emitAppTypes(app.id, contract, true) },
      // the backend's
      ...backendTypes(project, app, contract, dependencies),
      // the kit's tags (`<ui-button>`, ...), for the frameworks the page uses
      ...kitFiles(project),
      ...dependencies.map((d) => ({ path: `${GENERATED}/${d.id}.d.ts`, contents: emitAppTypes(d.id, d.contract, false) }))
    ]
  }
}

/** Write what changed (an unchanged file is not touched, so a watcher does not loop). */
function write(project: string, files: { path: string; contents: string }[]): void {
  for (const { path, contents } of files) {
    const target = join(project, path)
    mkdirSync(dirname(target), { recursive: true })
    if (existsSync(target) && readFileSync(target, 'utf8') === contents) continue
    writeFileSync(target, contents)
  }
}

/** The backend's sources under `dir`: its language's files. */
function sourcesUnder(dir: string, extension: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() ? sourcesUnder(path, extension) : name.endsWith(extension) ? [path] : []
  })
}

export function vos(options: Options = {}): Plugin {
  let root = process.cwd()
  let outDir = resolve(root, 'dist')
  let app: AppMeta | undefined
  let failed = false
  /** A TypeScript backend as this build compiled it when it started (the compiler runs once per build). */
  let compiled: string | undefined

  const regenerate = (): void => {
    const generated = generate(root, options)
    write(root, generated.files)
    app = generated.app
  }

  return {
    name: 'vos',
    config(config) {
      root = resolve(config.root ?? process.cwd())
      outDir = resolve(root, config.build?.outDir ?? 'dist')
      return {
        // relative URLs: the page is served at `/apps/<id>/static/`, wherever the host mounts it
        base: './',
        build: { outDir: join(outDir, STATIC_DIR), emptyOutDir: true, modulePreload: { polyfill: false } }
      }
    },
    configResolved() {
      regenerate()
    },
    transformIndexHtml: {
      order: 'post',
      handler: (html) => ({ html, tags: HOST_TAGS })
    },
    buildStart() {
      failed = false
      compiled = undefined
      for (const file of [CONTRACT, 'package.json']) this.addWatchFile(resolve(root, file))
      for (const path of options.dependencies ?? []) for (const file of [CONTRACT, 'package.json']) this.addWatchFile(resolve(root, path, file))
      const backend = backendOf(root)
      for (const file of sourcesUnder(resolve(root, 'server'), backend.language === 'luau' ? '.luau' : '.ts')) this.addWatchFile(file)
      regenerate()
      // a TypeScript backend is checked by compiling it, before the page is built: a type error is a failed build
      if (backend.language === 'typescript') compiled = compileBackend(root, GENERATED)
    },
    buildEnd(error) {
      if (error) failed = true
    },
    closeBundle() {
      if (failed || app === undefined || !existsSync(join(outDir, STATIC_DIR, 'index.html'))) return
      writeApp(app, outDir, compiled ?? bundleLuau(resolve(root, backendOf(root).path)), STATIC_DIR, readFileSync(resolve(root, CONTRACT)))
    }
  }
}
