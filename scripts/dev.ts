#!/usr/bin/env bun
/**
 * `bun run dev`: the host on :7400 (watching the built apps and reloading them), the client on :5173 through
 * Vite with hot reload, and the apps built into dist/apps (the host's default registry).
 *
 * Open http://127.0.0.1:5173 and pair with the code the host prints. Rebuild an app with `bun run build:apps`:
 * the host rebuilds its VM and keeps its data.
 */
import { spawn, spawnSync } from 'node:child_process'
import { resolve } from 'node:path'

const root = resolve(import.meta.dir, '..')

const built = spawnSync('bun', ['scripts/build-apps.ts'], { cwd: root, stdio: 'inherit' })
if (built.status !== 0) process.exit(built.status ?? 1)

const host = spawn('cargo', ['run', '-q', '-p', 'agentos-host'], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, AGENTOS_DEV_CLIENT_PORT: '5173', AGENTOS_DEV_RELOAD: '1' }
})
const client = spawn('bun', ['run', '--filter', '@agentos/client', 'dev'], { cwd: root, stdio: 'inherit' })

const stop = (): void => {
  host.kill('SIGTERM')
  client.kill('SIGTERM')
}
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, stop)
for (const child of [host, client]) child.on('exit', (code) => (stop(), process.exit(code ?? 0)))
