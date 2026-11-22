/**
 * The frame's token sheet (`/kit/tokens.css`): the shell's own `tokens.css` and `base.css`, joined. The kit does not keep
 * a second copy of the values, so an app frame and the shell cannot drift apart; `packages/kit/src/tokens.test.ts` holds
 * every `var(--x)` a component reads to this output.
 */
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const STYLES = resolve(import.meta.dirname, '../packages/ui/src/styles')

/**
 * Settings the host puts on the frame's document drive the tokens, so a component (inside its shadow root, where the
 * page's own rules do not reach) follows them through custom properties. Reduce Motion: `data-reduce-motion` on `<html>`
 * zeroes the motion tokens, which every component's transitions read, so none starts (the system setting, the media query,
 * is handled in each component's own styles).
 */
const HOOKS = `/* Reduce Motion set on the document (the host's setting): no transition starts. */
:root[data-reduce-motion] {
  --duration-fast: 0s;
  --duration: 0s;
  --duration-slow: 0s;
}`

export function buildTokensCss(): string {
  const part = (file: string): string => readFileSync(join(STYLES, file), 'utf8').trimEnd()
  return [
    '/* vos app kit: design tokens and the base reset. Generated from packages/ui/src/styles; do not edit. */',
    part('tokens.css'),
    part('base.css'),
    HOOKS,
    ''
  ].join('\n\n')
}
