#!/usr/bin/env bun
/**
 * `bun run dev` (and `bun run dev:vos`): the host on :7400, the client on :5173 through Vite, and everything the two
 * serve rebuilt when its source changes.
 *
 * Open http://127.0.0.1:5173 and pair with the code the host prints. What is kept fresh, and how:
 *
 *   apps/<app>/**              that app is rebuilt into dist/apps (its page and its Luau backend are one build); the
 *                              host watches the built refs there and rebuilds the app's VM, keeping its data
 *   packages/vite-plugin/src   every app is rebuilt (the plugin is what builds them)
 *   packages/kit/src           the kit is rebuilt into client/public/kit, which Vite serves
 *   crates/**, Cargo.*         the host is rebuilt, then restarted; a build that fails leaves the running host alone
 *   client/**, packages/ui     Vite's own dev server
 *
 * Nothing is pushed into an open app frame: reload the page (or reopen the app) to see a rebuilt app or kit.
 */
import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { watch, type FSWatcher } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dir, '..')
const hostBinary = join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug', 'vos')
const HOST_BUILD = ['cargo', 'build', '-q', '-p', 'vos-host']

// an app that does not build is not a reason to run nothing: the others are built, and it is rebuilt when it changes
if (spawnSync('bun', ['scripts/build-apps.ts'], { cwd: root, stdio: 'inherit' }).status !== 0)
  console.error('[dev] apps: not every app built; carrying on with the ones that did')
for (const command of [['bun', 'run', 'build:kit'], HOST_BUILD]) {
  const [bin = '', ...args] = command
  const done = spawnSync(bin, args, { cwd: root, stdio: 'inherit' })
  if (done.status !== 0) process.exit(done.status ?? 1)
}

/** What a build writes, and what is not source: a change there is not a change to build from. */
const GENERATED = /(^|\/)(\.app|dist|node_modules|storybook-static|e2e|test-results|target)\/|\.(test|stories)\.|(^|\/)\.[^/]*$|~$/

/**
 * One build at a time, a change during a build queues one more. `command` is asked when the build starts, so it can
 * take everything that changed since the last one.
 */
function rebuilder(name: string, command: () => string[], after?: () => void): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined
  let building = false
  let queued = false
  const run = (): void => {
    if (building) {
      queued = true
      return
    }
    building = true
    const [bin = '', ...args] = command()
    spawn(bin, args, { cwd: root, stdio: 'inherit' }).on('exit', (code) => {
      building = false
      if (code !== 0) console.error(`[dev] ${name}: the build failed; what was running keeps running`)
      else if (!queued) after?.()
      if (queued) {
        queued = false
        run()
      }
    })
  }
  return () => {
    clearTimeout(timer)
    timer = setTimeout(run, 200)
  }
}

// --- the host: built first, then run as the binary, so a restart stops the host itself (not a `cargo run` around it)
let host: ChildProcess | undefined
let stopping = false
let restarting = false

function startHost(): void {
  host = spawn(hostBinary, [], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, VOS_DEV_CLIENT_PORT: '5173', VOS_DEV_RELOAD: '1' }
  })
  host.on('exit', (code) => {
    if (stopping) return
    if (restarting) {
      restarting = false
      startHost()
      return
    }
    stop()
    process.exit(code ?? 0)
  })
}

/** The new binary is built: stop the running host and start the new one when it has gone (it holds the port). */
function restartHost(): void {
  if (stopping || restarting || host === undefined) return
  console.log('[dev] host: rebuilt, restarting')
  restarting = true
  host.kill('SIGTERM')
}

// --- the apps: only the ones that changed, or all of them after a change to what builds them
const changedApps = new Set<string>()
let everyApp = false
const rebuildApps = rebuilder('apps', () => {
  const ids = everyApp ? [] : [...changedApps]
  everyApp = false
  changedApps.clear()
  return ['bun', 'scripts/build-apps.ts', ...ids]
})
const rebuildKit = rebuilder('kit', () => ['bun', 'run', 'build:kit'])
const rebuildHost = rebuilder('host', () => HOST_BUILD, restartHost)

const watchers: FSWatcher[] = []
/** Watch a folder (or a file) under the root; `changed` gets the path relative to it, of source files only. */
function onChange(path: string, changed: (file: string) => void): void {
  watchers.push(
    watch(join(root, path), { recursive: true }, (_event, file) => {
      if (file === null || GENERATED.test(file)) return
      changed(file)
    })
  )
}

onChange('apps', (file) => {
  const app = file.split('/')[0]
  if (app === undefined || app === file) return
  changedApps.add(app)
  rebuildApps()
})
onChange('packages/vite-plugin/src', () => {
  everyApp = true
  rebuildApps()
})
onChange('packages/kit/src', rebuildKit)
onChange('crates', (file) => {
  if (/\.rs$|(^|\/)Cargo\.toml$/.test(file)) rebuildHost()
})
for (const manifest of ['Cargo.toml', 'Cargo.lock']) onChange(manifest, rebuildHost)

startHost()
const client = spawn('bun', ['run', '--filter', '@vos/client', 'dev'], { cwd: root, stdio: 'inherit' })
client.on('exit', (code) => {
  if (stopping) return
  stop()
  process.exit(code ?? 0)
})

function stop(): void {
  stopping = true
  for (const watcher of watchers) watcher.close()
  host?.kill('SIGTERM')
  client.kill('SIGTERM')
}
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, stop)
