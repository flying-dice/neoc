/**
 * Opening a link the person confirmed. Web pages open in a new tab without the shell as opener (the page never gets
 * `window.opener`, and no referrer leaks the host's address); a `mailto:` hands over to the mail client without
 * leaving the shell. Anything else is refused: this is the last check before the browser acts.
 */
import { describeLink } from '../view/core/link'

export interface LinkTarget {
  open(url: string, target: string, features: string): unknown
  location: { assign(url: string): void }
}

/** Returns whether the link was handed to the browser. */
export function openExternal(url: string, target: LinkTarget = window): boolean {
  const link = describeLink(url)
  if (!link) return false
  if (link.kind === 'mail') target.location.assign(link.url)
  else target.open(link.url, '_blank', 'noopener,noreferrer')
  return true
}
