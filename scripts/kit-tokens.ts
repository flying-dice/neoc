/**
 * The frame's token sheet (`/kit/tokens.css`): the shell's own `tokens.css` and `base.css`, joined. The kit does not keep
 * a second copy of the values, so an app frame and the shell cannot drift apart; `packages/kit/src/tokens.test.ts` holds
 * every `var(--x)` a component reads to this output.
 */
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const STYLES = resolve(import.meta.dirname, '../packages/ui/src/styles')

/** The bundled font file, which the kit's build writes next to the sheet, at `/kit/fonts/` (fonts.css). */
export const FONT_FILE = resolve(STYLES, '../fonts/InterVariable.woff2')

/**
 * Settings the host puts on the frame's document drive the tokens, so a component (inside its shadow root, where the
 * page's own rules do not reach) follows them through custom properties. Reduce Motion: `data-reduce-motion` on `<html>`
 * zeroes the motion tokens, which every component's transitions read, so none starts (the system setting, the media query,
 * is handled in each component's own styles). A subtree marked `data-theme="system"` follows `prefers-color-scheme`.
 */
const HOOKS = `/* A subtree that says "follow the system setting" (the document itself does so when it has no data-theme). */
[data-theme='system'] {
  color-scheme: light dark;
}

/* An app's window is above the desktop, so its content is on the elevated backgrounds, not the desktop's (in dark: gray6, not
   black). The names stay what an app reads; only what they are changes. The secondary and tertiary steps go one up as well,
   so a row, a card or a field still stands out from what it sits on. */
:root,
[data-theme] {
  --bg: var(--bg-elevated);
  --bg-secondary: light-dark(var(--ios-gray6), var(--ios-gray5));
  --bg-tertiary: light-dark(#ffffff, var(--ios-gray4));
  --bg-grouped: var(--bg-grouped-elevated);
  --bg-grouped-secondary: var(--bg-grouped-elevated-secondary);
  --bg-grouped-tertiary: light-dark(var(--ios-gray6), var(--ios-gray4));
}

/* Reduce Motion set on the document (the host's setting): no transition starts. */
:root[data-reduce-motion] {
  --duration-fast: 0s;
  --duration: 0s;
  --duration-slow: 0s;
}`

/**
 * The sheet. `fonts` is where the font file is, relative to the sheet (`fonts/` for `/kit/tokens.css`); without it
 * the sheet has no `@font-face`, and whoever loads it loads `fonts.css` itself (the kit's Storybook, through Vite).
 */
export function buildTokensCss(fonts?: string): string {
  const part = (file: string): string => readFileSync(join(STYLES, file), 'utf8').trimEnd()
  return [
    '/* vos app kit: design tokens and the base reset. Generated from packages/ui/src/styles; do not edit. */',
    ...(fonts === undefined ? [] : [part('fonts.css').replaceAll("url('../fonts/", `url('${fonts}`)]),
    part('tokens.css'),
    part('base.css'),
    HOOKS,
    ''
  ].join('\n\n')
}
