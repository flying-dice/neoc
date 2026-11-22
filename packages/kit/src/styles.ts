import { css } from 'lit'

/**
 * Styles every component starts from. A shadow root is a style boundary: the page's `*` rules do not reach in, so what
 * base.css does for the page (box sizing, the focus ring, Reduce Motion) is said again here for each component.
 * Tokens are custom properties and pass through the boundary, which is what lets the theme reach inside.
 */
export const base = css`
  :host {
    box-sizing: border-box;
  }
  *,
  *::before,
  *::after {
    box-sizing: inherit;
  }
  :focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }
  /* Reduce Motion: nothing moves, a style change applies at once (see packages/ui/src/styles/base.css). */
  @media (prefers-reduced-motion: reduce) {
    *,
    *::before,
    *::after {
      animation-duration: 0s !important;
      transition-duration: 0s !important;
    }
  }
`
