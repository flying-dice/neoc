/**
 * The vos Vite plugin, v2. An app is one Vite project:
 *
 *   - `package.json` says what the app is: its id (the package name without its scope), version, description, and under
 *     `vos` its title, icon, tint and window (`app.ts`);
 *   - `openrpc.json` (OpenRPC 1.4) types everything the backend serves and sends (`contract.ts`);
 *   - `server/main.luau` is the backend: it returns `rpc.serve(handlers)`;
 *   - the page is a normal Vite page whose API is the global `vos` (the host's bridge defines it): nothing to import.
 *
 *     // vite.config.ts
 *     import { vos } from '@vos/vite-plugin-v2'
 *     export default defineConfig({ plugins: [svelte(), vos()] })
 *
 * Another app it talks to is a dependency, by where it is (`vos({ dependencies: ['../app1'] })`): a folder holding that
 * app's package.json and openrpc.json. The page's client then types that app's public notifications under its id.
 *
 * The plugin writes `types/` (`rpc.d.ts` and `rpc.js` for the page, `rpc.luau` for the backend) whenever Vite resolves
 * its config, so every Vite command leaves it current; puts the host's tags first in the page; and on `vite build`
 * bundles the backend into `dist/app.luau` and writes `dist/app-ref.json`, the app ref the host installs. Checking,
 * linting and formatting the Luau are the Luau tools' (luau-lsp, StyLua), not the plugin's.
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import type { HtmlTagDescriptor, Plugin } from 'vite'
import { parseApp, type AppMeta } from './app.ts'
import { bundleLuau, writeApp } from './build.ts'
import { parseContract } from './contract.ts'
import { emitAppTypes, emitVosTypes, type Dependency } from './emit-ts.ts'
import { frameworksOf, kitTypes, type Manifest } from './kit-types.ts'
import { createRequire } from 'node:module'
import { emitDependencyLuau, emitLuau } from './emit-luau.ts'

export { parseApp, AppError, type AppMeta } from './app.ts'
export { parseContract, ContractError, type Contract } from './contract.ts'
export type { Dependency } from './emit-ts.ts'

export const CONTRACT = 'openrpc.json'
export const BACKEND = 'server/main.luau'
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
      // types only: the page's global `vos` and the apps it knows (its own, each it depends on), and the backend's
      // modules of types (its own app's, each it depends on); nothing here runs
      { path: `${GENERATED}/vos.d.ts`, contents: emitVosTypes() },
      { path: `${GENERATED}/${app.id}.d.ts`, contents: emitAppTypes(app.id, contract, true) },
      { path: `${GENERATED}/${app.id}.luau`, contents: emitLuau(contract, CONTRACT, app.id) },
      // the kit's tags (`<ui-button>`, ...), for the frameworks the page uses
      ...frameworksOf(JSON.parse(readFileSync(join(project, 'package.json'), 'utf8')) as Parameters<typeof frameworksOf>[0]).map((framework) => ({
        path: `${GENERATED}/kit-${framework}.d.ts`,
        contents: kitTypes(readKitManifest(), framework)
      })),
      ...dependencies.flatMap((d) => [
        { path: `${GENERATED}/${d.id}.d.ts`, contents: emitAppTypes(d.id, d.contract, false) },
        { path: `${GENERATED}/${d.id}.luau`, contents: emitDependencyLuau(d) }
      ])
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

function sourcesUnder(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() ? sourcesUnder(path) : name.endsWith('.luau') ? [path] : []
  })
}

export function vos(options: Options = {}): Plugin {
  let root = process.cwd()
  let outDir = resolve(root, 'dist')
  let app: AppMeta | undefined
  let failed = false

  const regenerate = (): void => {
    const generated = generate(root, options)
    write(root, generated.files)
    app = generated.app
  }

  return {
    name: 'vos-v2',
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
      for (const file of [CONTRACT, 'package.json']) this.addWatchFile(resolve(root, file))
      for (const path of options.dependencies ?? []) for (const file of [CONTRACT, 'package.json']) this.addWatchFile(resolve(root, path, file))
      for (const file of sourcesUnder(resolve(root, 'server'))) this.addWatchFile(file)
      regenerate()
      if (!existsSync(resolve(root, BACKEND))) throw new Error(`${BACKEND} not found: the backend returns rpc.serve(handlers)`)
    },
    buildEnd(error) {
      if (error) failed = true
    },
    closeBundle() {
      if (failed || app === undefined || !existsSync(join(outDir, STATIC_DIR, 'index.html'))) return
      writeApp(app, outDir, bundleLuau(resolve(root, BACKEND)), STATIC_DIR, readFileSync(resolve(root, CONTRACT)))
    }
  }
}
