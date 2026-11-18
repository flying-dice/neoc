/**
 * After the host is up: pair one browser, the way a person does (a code from `agentos-host pair`, POST /pair),
 * and save its cookie as the Playwright storage state the desktop and journey specs share.
 */
import { writeFileSync } from 'node:fs'
import { STATE } from './config'
import { pairDevice } from './pair'

export default async function globalSetup(): Promise<void> {
  const { cookie } = await pairDevice('Playwright')
  writeFileSync(STATE, JSON.stringify({ cookies: [{ ...cookie, expires: -1, secure: false }], origins: [] }))
}
