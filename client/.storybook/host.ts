/**
 * What built-in page stories run against, installed before any story is imported:
 *
 *  - the shell's host connection is a fake (`shell/fakeHost`): installed apps, a registry catalog, app refs to
 *    preview and paired devices, with the host's own answers. The real host is exercised by the Playwright
 *    journeys, not here.
 *  - the Agents page still reads the old SDK bridge (wave B3 replaces it with the AG-UI Agents app), so its
 *    stories keep the in-memory host with a scripted agent.
 */
import { createMemoryHost, demoAgent, installHost } from '@agentos/sdk/testing'
import { host } from '../src/renderer/src/lib/host.svelte'
import { createFakeHost, FAKE_CALCULATOR } from '../src/renderer/src/shell/fakeHost'

installHost(
  createMemoryHost({ apps: [], agents: [demoAgent()], versions: { client: 'storybook' } })
)
host.useFake(createFakeHost(), [FAKE_CALCULATOR])
