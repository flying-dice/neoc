/**
 * What the shell's stories run against, installed before any story is imported: the shell's host connection is a
 * fake (`shell/fakeHost`): installed apps, a registry catalog, app refs to preview, paired devices and the agent
 * roster, with the host's own answers. The real host is exercised by the Playwright journeys, not here.
 */
import { host } from '../src/lib/host.svelte'
import { createFakeHost, FAKE_CALCULATOR } from '../src/shell/fakeHost'

host.useFake(createFakeHost(), [FAKE_CALCULATOR])
