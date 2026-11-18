/**
 * Where the end-to-end host lives. One host serves every spec: a fresh data directory under test-results/,
 * the built client, a test registry, a fixed port that is not the default (7400), so it never collides with a
 * host a person has running.
 */
import { join, resolve } from 'node:path'

export const ROOT = resolve(import.meta.dirname, '../..')
export const PORT = 7410
/** `127.0.0.1`, not `localhost`: the session cookie is host-only, so every URL must use the same name. */
export const ORIGIN = `http://127.0.0.1:${PORT}`
export const DIR = join(ROOT, 'test-results/host')
export const DATA = join(DIR, 'data')
export const CONFIG = join(DIR, 'agentos.toml')
export const REGISTRY = join(DIR, 'registry/registry.json')
/** The paired browser the specs share (a Playwright storage state). */
export const STATE = join(DIR, 'paired.json')
export const HOST_BIN = join(ROOT, 'target/debug/agentos-host')
/** The scripted ACP agent (no model): the prompt text picks what it does. */
export const FAKE_AGENT = join(ROOT, 'target/debug/fake_agent')
