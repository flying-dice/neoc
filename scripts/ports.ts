/**
 * Every port the end-to-end run uses, from one env var so two checkouts can run it side by side:
 *
 *   VOS_PORT_BASE=9100 bun run test:e2e     Storybooks on 9100, the e2e host on 9110
 *
 * Unset, the ports are the long-standing defaults: Storybooks on 6100, the e2e host on 7410 (never 7400,
 * so it cannot collide with a host a person has running).
 */
const DEFAULT_STORYBOOKS = 6100
const HOST_OFFSET = 10
const DEFAULT_HOST = 7410

const raw = process.env.VOS_PORT_BASE
const base = raw === undefined || raw === '' ? undefined : Number(raw)
if (base !== undefined && (!Number.isInteger(base) || base < 1024 || base + HOST_OFFSET > 65_535)) {
  throw new Error(`VOS_PORT_BASE must be an integer between 1024 and ${65_535 - HOST_OFFSET}, got "${raw}"`)
}

export const STORYBOOKS_PORT = base ?? DEFAULT_STORYBOOKS
export const HOST_PORT = base === undefined ? DEFAULT_HOST : base + HOST_OFFSET
