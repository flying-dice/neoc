/**
 * `bun apps/server/src/main.ts`: the AgentOS server. Configure with env vars
 * (see config.ts): AGENTOS_PORT, AGENTOS_HOST, AGENTOS_DATA, AGENTOS_REGISTRIES,
 * AGENTOS_ALLOWED_ORIGINS, AGENTOS_DEV=1.
 */
import { watch } from 'node:fs'
import { loadConfig } from './config'
import { startServer } from './server'

const config = loadConfig()
const server = await startServer(config)

if (config.dev) {
  // Rebuilding the local registry (bun run build:apps) hot-reloads installed apps.
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    watch(config.localRegistryDir, { recursive: false }, (_event, file) => {
      if (file !== 'registry.json') return
      clearTimeout(timer)
      timer = setTimeout(async () => {
        const reloaded = await server.syncLocal().catch((e: unknown) => {
          console.error('[agentos] reload failed:', e)
          return []
        })
        if (reloaded.length) console.log(`[agentos] reloaded ${reloaded.join(', ')}`)
      }, 200)
    })
  } catch {
    console.warn(`[agentos] no local registry at ${config.localRegistryDir} yet: run \`bun run build:apps\``)
  }
}

const shutdown = async (): Promise<void> => {
  await server.stop()
  process.exit(0)
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
