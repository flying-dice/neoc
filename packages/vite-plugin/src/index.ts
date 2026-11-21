/**
 * The vos Vite plugin. An app is one Vite project: its client in any framework, its Luau backend beside it, and one
 * RPC definition (`rpc.openrpc.json`, OpenRPC / JSON-RPC 2.0) that both sides are generated from.
 *
 *     // vite.config.ts
 *     import { vos } from '@vos/vite-plugin'
 *     export default defineConfig({ plugins: [svelte(), vos()] })
 *
 * What it does:
 *   - generates `generated/rpc.ts` (the typed client) and `generated/rpc.luau` (the types, the handler interface the
 *     developer implements, and the router) from the definition, when the build or the dev server starts and whenever the
 *     definition changes;
 *   - builds the client into one classic script that defines `vosApp.mount` (`dist/client.js`);
 *   - bundles the Luau backend into `dist/app.luau` and writes `dist/app-ref.json`, the app ref the host installs.
 */
import { resolve } from 'node:path'
import type { Plugin } from 'vite'
import { writeApp } from './backend.ts'
import { DEFAULTS, generate, stale, write, type Options } from './generate.ts'

export type { Options } from './generate.ts'
export { DefinitionError, parseDefinition } from './definition.ts'

export function vos(options: Partial<Options> = {}): Plugin {
  const settings: Options = { ...DEFAULTS, ...options }
  let root = process.cwd()
  let outDir = 'dist'
  const client = 'client.js'

  const regenerate = (): string[] => write(root, generate(root, settings))

  return {
    name: 'vos',
    config(config) {
      root = resolve(config.root ?? process.cwd())
      outDir = config.build?.outDir ?? 'dist'
      return {
        // a classic script, not a module: an app's frame has an opaque origin, where a module script needs CORS
        build: {
          lib: { entry: resolve(root, settings.client), name: 'vosApp', formats: ['iife'], fileName: () => client },
          outDir,
          emptyOutDir: true,
          cssCodeSplit: false
        },
        // a library build leaves `process.env.NODE_ENV` to the page, and a frame has no `process`
        define: { 'process.env.NODE_ENV': JSON.stringify('production') }
      }
    },
    buildStart() {
      regenerate()
      this.addWatchFile(resolve(root, settings.definition))
    },
    configureServer(server) {
      const definition = resolve(root, settings.definition)
      regenerate()
      server.watcher.add(definition)
      server.watcher.on('change', (file) => {
        if (file === definition) regenerate()
      })
    },
    closeBundle() {
      writeApp(root, resolve(root, outDir), settings.backend, client)
    }
  }
}

/** The generated files that are out of date (for `vos generate --check`). */
export function staleFiles(project: string, options: Partial<Options> = {}): string[] {
  const settings: Options = { ...DEFAULTS, ...options }
  return stale(project, generate(project, settings))
}
