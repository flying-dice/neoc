/**
 * `bun apps/server/src/main.ts`: the AgentOS server. Configure with env vars
 * (see config.ts): AGENTOS_PORT, AGENTOS_HOST, AGENTOS_DATA, AGENTOS_KNOWN_APPS,
 * AGENTOS_APPS, AGENTOS_ALLOWED_ORIGINS, AGENTOS_DEV=1.
 */
import { watch } from 'node:fs'
import { dirname } from 'node:path'
import { loadConfig } from './config'
import { startServer } from './server'

const config = loadConfig()
const server = await startServer(config)

if (config.dev) {
  // `bun run build:apps` rewrites the known-apps file last: reload what changed.
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    watch(dirname(config.knownAppsFile), { recursive: false }, (_event, file) => {
      if (!file || !config.knownAppsFile.endsWith(file)) return
      clearTimeout(timer)
      timer = setTimeout(async () => {
        const reloaded = await server.reloadChanged().catch((e: unknown) => {
          console.error('[agentos] reload failed:', e)
          return []
        })
        if (reloaded.length) console.log(`[agentos] reloaded ${reloaded.join(', ')}`)
      }, 200)
    })
  } catch {
    console.warn(`[agentos] can't watch ${config.knownAppsFile}: run \`bun run build:apps\`, then restart`)
  }
}

const shutdown = async (): Promise<void> => {
  await server.stop()
  process.exit(0)
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
