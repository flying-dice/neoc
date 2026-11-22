/**
 * The frame's token sheet (`/kit/tokens.css`): the shell's own `tokens.css` and `base.css`, joined. The kit does not keep
 * a second copy of the values, so an app frame and the shell cannot drift apart; `packages/kit/src/tokens.test.ts` holds
 * every `var(--x)` a component reads to this output.
 */
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const STYLES = resolve(import.meta.dirname, '../packages/ui/src/styles')

export function buildTokensCss(): string {
  const part = (file: string): string => readFileSync(join(STYLES, file), 'utf8').trimEnd()
  return [
    '/* vos app kit: design tokens and the base reset. Generated from packages/ui/src/styles; do not edit. */',
    part('tokens.css'),
    part('base.css'),
    ''
  ].join('\n\n')
}
