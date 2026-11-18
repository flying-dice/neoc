/**
 * Builds an app module into a registry bundle (runs under Bun):
 *
 *   <out>/<id>/<version>/manifest.json
 *   <out>/<id>/<version>/server.js   ESM for the Bun server (bun:sqlite and any Bun API allowed)
 *   <out>/<id>/<version>/client.js   browser ESM; Svelte/kit/SDK come from the host (SHARED_MODULES)
 *
 * App layout (by convention): src/manifest.ts (default export), src/server/index.ts
 * (default export defineServerApp), src/client/index.ts (default export defineClientApp).
 */
import { compile, compileModule } from 'svelte/compiler'
import { dirname, join, relative } from 'node:path'
import { mkdir, readFile } from 'node:fs/promises'
import type { BunPlugin } from 'bun'
import type { AppManifest } from '../manifest'
import { manifestSchema, registrySchema, type Registry, type RegistryEntry } from '../registry'
import { SHARED_GLOBAL, SHARED_MODULES, SHARED_SIDE_EFFECTS } from './shared'

export { SHARED_GLOBAL, SHARED_MODULES, SHARED_SIDE_EFFECTS } from './shared'

const escape = (s: string): string => s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')
const exactly = (specs: readonly string[]): RegExp => new RegExp(`^(${specs.map(escape).join('|')})$`)
const IDENT = /^[A-Za-z_$][\w$]*$/

/** Compile .svelte components and .svelte.ts/.js rune modules for the browser. */
export function sveltePlugin(): BunPlugin {
  return {
    name: 'agentos-svelte',
    setup(build) {
      build.onLoad({ filter: /\.svelte$/ }, async ({ path }) => {
        const result = compile(await Bun.file(path).text(), {
          filename: path,
          generate: 'client',
          css: 'injected',
          runes: true,
          dev: false
        })
        return { contents: result.js.code, loader: 'js' }
      })
      build.onLoad({ filter: /\.svelte\.(ts|js)$/ }, async ({ path }) => {
        let code = await Bun.file(path).text()
        if (path.endsWith('.ts')) code = new Bun.Transpiler({ loader: 'ts' }).transformSync(code)
        const result = compileModule(code, { filename: path, generate: 'client', dev: false })
        return { contents: result.js.code, loader: 'js' }
      })
    }
  }
}

/** Export names of a shared module, as the browser sees it. */
async function exportNames(spec: string, from: string): Promise<string[]> {
  if (spec.startsWith('@agentos/')) {
    // Workspace packages ship TS/Svelte source: read the barrel's export list.
    const file = Bun.resolveSync(spec, from)
    const source = await readFile(file, 'utf8')
    const names = new Set<string>()
    for (const block of source.matchAll(/export\s*\{([^}]*)\}/g)) {
      for (const part of block[1].split(',')) {
        const item = part.trim()
        if (!item || item.startsWith('type ')) continue
        const name = item.split(/\s+as\s+/).pop()!.trim()
        if (IDENT.test(name)) names.add(name)
      }
    }
    for (const decl of source.matchAll(/export\s+(?:const|let|function|class)\s+([A-Za-z_$][\w$]*)/g)) names.add(decl[1])
    return [...names]
  }
  // npm packages: import the browser entry so names match what the client loads.
  const [pkg, ...rest] = spec.split('/')
  const pkgJsonPath = Bun.resolveSync(`${pkg}/package.json`, from)
  const pkgJson = JSON.parse(await readFile(pkgJsonPath, 'utf8')) as { exports?: Record<string, unknown> }
  const subpath = rest.length ? `./${rest.join('/')}` : '.'
  const entry = pkgJson.exports?.[subpath]
  const pick = (e: unknown): string | undefined =>
    typeof e === 'string' ? e : e && typeof e === 'object' ? pick((e as Record<string, unknown>).browser ?? (e as Record<string, unknown>).default) : undefined
  const target = pick(entry)
  const mod = (await import(target ? join(dirname(pkgJsonPath), target) : spec)) as Record<string, unknown>
  return Object.keys(mod).filter((k) => IDENT.test(k) || k === 'default')
}

/** Replace shared imports with reads from the host's shared module table. */
export async function sharedModulesPlugin(from: string): Promise<BunPlugin> {
  const names = new Map<string, string[]>()
  for (const spec of SHARED_MODULES) names.set(spec, await exportNames(spec, from))
  return {
    name: 'agentos-shared',
    setup(build) {
      build.onResolve({ filter: exactly(SHARED_SIDE_EFFECTS) }, ({ path }) => ({ path, namespace: 'agentos-empty' }))
      build.onLoad({ filter: /.*/, namespace: 'agentos-empty' }, () => ({ contents: '', loader: 'js' }))
      build.onResolve({ filter: exactly(SHARED_MODULES) }, ({ path }) => ({ path, namespace: 'agentos-shared' }))
      build.onLoad({ filter: /.*/, namespace: 'agentos-shared' }, ({ path }) => {
        const keys = names.get(path) ?? []
        // Bind each export privately, then re-export it under its real name:
        // export names may be reserved words (Svelte's runtime exports `if`, `await`, `each`).
        const named = keys.filter((k) => k !== 'default')
        const lines = [
          `const m = globalThis[${JSON.stringify(SHARED_GLOBAL)}]?.[${JSON.stringify(path)}];`,
          `if (!m) throw new Error(${JSON.stringify(`AgentOS shared module "${path}" is unavailable: load this bundle inside the AgentOS client`)});`,
          ...named.map((k, i) => `const __shared${i} = m[${JSON.stringify(k)}];`),
          named.length ? `export { ${named.map((k, i) => `__shared${i} as ${k}`).join(', ')} };` : '',
          ...(keys.includes('default') ? ['export default m.default;'] : [])
        ]
        return { contents: lines.join('\n'), loader: 'js' }
      })
    }
  }
}

async function sha256(file: string): Promise<string> {
  return new Bun.CryptoHasher('sha256').update(await Bun.file(file).arrayBuffer()).digest('hex')
}

async function buildOne(options: Parameters<typeof Bun.build>[0], label: string): Promise<void> {
  const result = await Bun.build(options)
  if (!result.success) {
    throw new Error(`${label} build failed:\n${result.logs.map((l) => String(l)).join('\n')}`)
  }
  const extra = result.outputs.filter((o) => o.kind !== 'entry-point')
  if (extra.length) {
    throw new Error(`${label} must build to a single file; got extra outputs: ${extra.map((o) => o.path).join(', ')}`)
  }
}

export interface BuildAppOptions {
  /** The app package directory (contains src/manifest.ts). */
  appDir: string
  /** Registry output directory; bundles go to <outDir>/<id>/<version>/. */
  outDir: string
  minify?: boolean
}

/** Build one app; returns its registry entry (URLs relative to outDir). */
export async function buildAppBundle({ appDir, outDir, minify = true }: BuildAppOptions): Promise<RegistryEntry> {
  const manifestModule = (await import(join(appDir, 'src/manifest.ts'))) as { default: AppManifest }
  const manifest = manifestSchema.parse(manifestModule.default) as AppManifest
  const dir = join(outDir, manifest.id, manifest.version)
  await mkdir(dir, { recursive: true })

  await buildOne(
    {
      entrypoints: [join(appDir, 'src/server/index.ts')],
      outdir: dir,
      naming: 'server.js',
      target: 'bun',
      format: 'esm',
      minify
    },
    `${manifest.id} server`
  )
  await buildOne(
    {
      entrypoints: [join(appDir, 'src/client/index.ts')],
      outdir: dir,
      naming: 'client.js',
      target: 'browser',
      format: 'esm',
      minify,
      plugins: [await sharedModulesPlugin(appDir), sveltePlugin()]
    },
    `${manifest.id} client`
  )
  await Bun.write(join(dir, 'manifest.json'), JSON.stringify(manifest, null, 2))

  const ref = async (name: string): Promise<{ url: string; sha256: string }> => ({
    url: relative(outDir, join(dir, name)).split('\\').join('/'),
    sha256: await sha256(join(dir, name))
  })
  return { manifest, server: await ref('server.js'), client: await ref('client.js') }
}

/**
 * Build a whole registry: every app directory → bundles + registry.json in
 * `outDir`. Old bundle versions in outDir are left in place (registries may
 * serve several versions); registry.json lists the ones just built.
 */
export async function buildRegistry(options: {
  name: string
  appDirs: string[]
  outDir: string
  minify?: boolean
  /** Keep entries for apps not rebuilt this time (partial builds). */
  merge?: boolean
}): Promise<Registry> {
  const built: RegistryEntry[] = []
  for (const appDir of options.appDirs) {
    built.push(await buildAppBundle({ appDir, outDir: options.outDir, minify: options.minify }))
  }
  let kept: RegistryEntry[] = []
  const existing = Bun.file(join(options.outDir, 'registry.json'))
  if (options.merge && (await existing.exists())) {
    const ids = new Set(built.map((e) => e.manifest.id))
    kept = registrySchema.parse(await existing.json()).apps.filter((e) => !ids.has(e.manifest.id))
  }
  const apps = [...kept, ...built].sort((a, b) => a.manifest.id.localeCompare(b.manifest.id))
  const registry = registrySchema.parse({ name: options.name, apps })
  await Bun.write(join(options.outDir, 'registry.json'), JSON.stringify(registry, null, 2))
  return registry
}
