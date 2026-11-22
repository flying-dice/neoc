import { styleMap } from 'lit/directives/style-map.js'

/** A value a `color` property accepts: a name, a hex, `var(--x)`, `color-mix(...)`; never `red;position:fixed`. */
export const isColour = (value: unknown): value is string =>
  typeof value === 'string' && value !== '' && CSS.supports('color', value)

/**
 * The inline style of an element that takes colours from its properties (`tint`, `fill`). Each is set as one custom
 * property through the style API, and only when it is a valid colour: a string from an app or an agent is data, so it can
 * never carry another declaration into the style.
 */
export const colourVars = (
  vars: Record<string, string | undefined>
): ReturnType<typeof styleMap> => {
  const valid: Record<string, string> = {}
  for (const [name, value] of Object.entries(vars)) if (isColour(value)) valid[name] = value
  return styleMap(valid)
}
