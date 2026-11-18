/**
 * Builds an app module into a static bundle directory (runs under Bun):
 *
 *   <out>/<id>/bundle.json             the latest version (URLs point into <version>/)
 *   <out>/<id>/<version>/bundle.json   that version: manifest + file URLs and sha256s
 *   <out>/<id>/<version>/server.js     ESM for the Bun server (bun:sqlite and any Bun API allowed)
 *   <out>/<id>/<version>/client.js     browser ESM; Svelte/kit/SDK come from the host (SHARED_MODULES)
 *
 * The directory is plain static files: commit it, or host it anywhere (GitLab
 * Pages, any static server) and add the bundle.json URL to a server's known apps.
 *
 * App layout (by convention): src/manifest.ts (default export), src/server/index.ts
 * (default export defineServerApp), src/client/index.ts (default export defineClientApp).
 */
import { compile, compileModule } from 'svelte/compiler'
import { dirname, join } from 'node:path'
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import type { BunPlugin } from 'bun'
import type { AppManifest } from '../manifest'
import { bundleDescriptorSchema, manifestSchema, type BundleDescriptor } from '../bundle'
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
  /** Bundles directory; the app goes to <outDir>/<id>/. */
  outDir: string
  minify?: boolean
  /**
   * Published versions are immutable: refuse to overwrite <id>/<version>/ with
   * different bytes (bump the version instead). Off for dev builds.
   */
  immutable?: boolean
}

export class PublishedVersionError extends Error {}

/** Build one app; returns its version descriptor (URLs relative to <id>/<version>/). */
export async function buildAppBundle({
  appDir,
  outDir,
  minify = true,
  immutable = false
}: BuildAppOptions): Promise<BundleDescriptor> {
  const manifestModule = (await import(join(appDir, 'src/manifest.ts'))) as { default: AppManifest }
  const manifest = manifestSchema.parse(manifestModule.default) as AppManifest
  const staging = await mkdtemp(join(tmpdir(), `agentos-${manifest.id}-`))
  try {
    await buildOne(
      {
        entrypoints: [join(appDir, 'src/server/index.ts')],
        outdir: staging,
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
        outdir: staging,
        naming: 'client.js',
        target: 'browser',
        format: 'esm',
        minify,
        plugins: [await sharedModulesPlugin(appDir), sveltePlugin()]
      },
      `${manifest.id} client`
    )
    const descriptor = bundleDescriptorSchema.parse({
      manifest,
      server: { url: 'server.js', sha256: await sha256(join(staging, 'server.js')) },
      client: { url: 'client.js', sha256: await sha256(join(staging, 'client.js')) }
    })

    const appRoot = join(outDir, manifest.id)
    const dir = join(appRoot, manifest.version)
    const published = Bun.file(join(dir, 'bundle.json'))
    if (immutable && (await published.exists())) {
      const existing = bundleDescriptorSchema.parse(await published.json())
      if (existing.server.sha256 !== descriptor.server.sha256 || existing.client.sha256 !== descriptor.client.sha256) {
        throw new PublishedVersionError(
          `${manifest.id}@${manifest.version} is already published with different contents. ` +
            `Bump "version" in ${join(appDir, 'src/manifest.ts')} to release a change.`
        )
      }
    }
    await mkdir(dir, { recursive: true })
    await Bun.write(join(dir, 'server.js'), Bun.file(join(staging, 'server.js')))
    await Bun.write(join(dir, 'client.js'), Bun.file(join(staging, 'client.js')))
    await Bun.write(join(dir, 'bundle.json'), JSON.stringify(descriptor, null, 2) + '\n')
    const latest = {
      ...descriptor,
      server: { ...descriptor.server, url: `${manifest.version}/server.js` },
      client: { ...descriptor.client, url: `${manifest.version}/client.js` }
    }
    await Bun.write(join(appRoot, 'bundle.json'), JSON.stringify(latest, null, 2) + '\n')
    return descriptor
  } finally {
    await rm(staging, { recursive: true, force: true })
  }
}
