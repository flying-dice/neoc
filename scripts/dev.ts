#!/usr/bin/env bun
/**
 * `bun run dev`: the host on :7400 (watching the built apps and reloading them), the client on :5173 through
 * Vite with hot reload, and the apps built into dist/apps (the host's default registry).
 *
 * Open http://127.0.0.1:5173 and pair with the code the host prints. Apps are authored in Luau and the host runs
 * the Lua 5.1 build of them (darklua, see scripts/lua51.ts), which only this script can make: it watches
 * `apps/**` (`*.luau`, `app.json`, the migrations) and re-runs `build:apps` when one changes. The host watches
 * the built refs under dist/apps (`app.lua`, `app-ref.json`) and rebuilds the app's VM, keeping its data.
 */
import { spawn, spawnSync } from 'node:child_process'
import { watch } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dir, '..')

const built = spawnSync('bun', ['scripts/build-apps.ts'], { cwd: root, stdio: 'inherit' })
if (built.status !== 0) process.exit(built.status ?? 1)

/** What an app is built from: its Luau, its manifest and its SQL migrations. */
const SOURCE = /\.(luau|sql)$|(^|\/)app\.json$/
let timer: ReturnType<typeof setTimeout> | undefined
let building = false
let queued = false

/** Rebuild every app (it also rewrites registry.json): one build at a time, a change during a build queues one more. */
function rebuild(): void {
  if (building) {
    queued = true
    return
  }
  building = true
  const run = spawn('bun', ['scripts/build-apps.ts'], { cwd: root, stdio: 'inherit' })
  run.on('exit', (code) => {
    building = false
    if (code !== 0) console.error('build:apps failed: the host keeps running the last good build')
    if (queued) {
      queued = false
      rebuild()
    }
  })
}

const sources = watch(join(root, 'apps'), { recursive: true }, (_event, file) => {
  if (file === null || !SOURCE.test(file)) return
  clearTimeout(timer)
  timer = setTimeout(rebuild, 150)
})

const host = spawn('cargo', ['run', '-q', '-p', 'agentos-host'], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, AGENTOS_DEV_CLIENT_PORT: '5173', AGENTOS_DEV_RELOAD: '1' }
})
const client = spawn('bun', ['run', '--filter', '@agentos/client', 'dev'], { cwd: root, stdio: 'inherit' })

const stop = (): void => {
  sources.close()
  clearTimeout(timer)
  host.kill('SIGTERM')
  client.kill('SIGTERM')
}
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, stop)
for (const child of [host, client]) child.on('exit', (code) => (stop(), process.exit(code ?? 0)))
