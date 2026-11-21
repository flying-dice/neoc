/**
 * `aria-description` (ARIA 1.3) is not in Svelte's element typings, so it is spread rather than written as an
 * attribute: `<div {...describe(description)}>`. Nothing is rendered when there is no description.
 */
export const describe = (description: string | undefined): Record<string, string> =>
  description ? { 'aria-description': description } : {}
