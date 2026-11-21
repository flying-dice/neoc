#!/usr/bin/env bun
/**
 * Every workspace package (`packages/*`) and the client with a `.storybook/` folder has its own Storybook.
 *
 *   bun scripts/storybooks.ts list     names, one per line
 *   bun scripts/storybooks.ts build    build them all (storybook-static/)
 *   bun scripts/storybooks.ts serve    serve them all on one port (building missing ones first):
 *                                      http://localhost:6100/<name>/ (VOS_PORT_BASE moves it)  and  /storybooks.json
 *
 * `bun run test:e2e` builds first, so the stories spec can read each
 * storybook-static/index.json; a new package with a `.storybook/` folder is picked up automatically.
 * FORCE_STORYBOOK_BUILD=1 rebuilds before serving.
 */
import { existsSync } from 'node:fs'
import { basename, join, resolve } from 'node:path'
import { Glob } from 'bun'
import { STORYBOOKS_PORT } from './ports'

const ROOT = resolve(import.meta.dir, '..')

interface Storybook {
  name: string
  dir: string
}

async function discover(): Promise<Storybook[]> {
  const found: Storybook[] = []
  for (const pattern of ['packages/*/.storybook/main.ts', 'client/.storybook/main.ts']) {
    for await (const main of new Glob(pattern).scan({ cwd: ROOT, dot: true })) {
      const dir = join(ROOT, main, '..', '..')
      found.push({ name: basename(dir), dir })
    }
  }
  return found.sort((a, b) => a.name.localeCompare(b.name))
}

async function build(books: Storybook[]): Promise<void> {
  const results = await Promise.all(
    books.map(async (b) => {
      const proc = Bun.spawn(['bunx', 'storybook', 'build', '-o', 'storybook-static', '--quiet'], {
        cwd: b.dir,
        stdout: 'pipe',
        stderr: 'pipe',
        env: { ...process.env, STORYBOOK_DISABLE_TELEMETRY: '1' }
      })
      const code = await proc.exited
      if (code !== 0) {
        console.error(`✗ ${b.name}: storybook build failed\n${await new Response(proc.stderr).text()}`)
      } else console.log(`✓ built ${b.name} Storybook`)
      return code
    })
  )
  if (results.some((c) => c !== 0)) process.exit(1)
}

function serve(books: Storybook[]): void {
  const byName = new Map(books.map((b) => [b.name, join(b.dir, 'storybook-static')]))
  Bun.serve({
    port: STORYBOOKS_PORT,
    async fetch(req) {
      const { pathname } = new URL(req.url)
      if (pathname === '/storybooks.json') return Response.json(books.map((b) => b.name))
      const [, name, ...rest] = pathname.split('/')
      const root = byName.get(name)
      if (!root) return new Response('Not found', { status: 404 })
      if (!rest.length) return Response.redirect(`/${name}/`, 302)
      const file = resolve(root, rest.join('/') || 'index.html')
      if (!file.startsWith(root)) return new Response('Forbidden', { status: 403 })
      const f = Bun.file(file)
      return (await f.exists()) ? new Response(f) : new Response('Not found', { status: 404 })
    }
  })
  console.log(`Serving ${books.map((b) => b.name).join(', ')} on http://localhost:${STORYBOOKS_PORT}`)
}

const books = await discover()
switch (process.argv[2]) {
  case 'list':
    for (const b of books) console.log(b.name)
    break
  case 'build':
    await build(books)
    break
  case 'serve':
    if (process.env.FORCE_STORYBOOK_BUILD || books.some((b) => !existsSync(join(b.dir, 'storybook-static')))) {
      await build(books)
    }
    serve(books)
    break
  default:
    console.error('Usage: bun scripts/storybooks.ts list|build|serve')
    process.exit(1)
}
