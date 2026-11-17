/**
 * Install the in-memory host before any store is imported (stores read the
 * bridge on first import), so built-in app stories run against the same
 * fake backend as web mode: real app main modules, a scripted Claude.
 */
import { createMemoryHost, demoAgent, installHost } from '@agentos/sdk/testing'
import { installedApps } from '../src/main/platform/installedApps'

installHost(
  createMemoryHost({
    apps: installedApps,
    agents: [demoAgent()],
    versions: { electron: '44.11.3', chrome: '146.0.7680.65' }
  })
)
