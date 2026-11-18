/**
 * What a window may ask the shell to open outside AgentOS (Markdown links, `view.command openUrl`), described for the
 * confirmation the person sees first. The text a link shows says nothing about where it goes, so the confirmation
 * names the host (read from the parsed URL, in its ASCII form: a look-alike domain shows as `xn--...`) and the full
 * address. Only web pages and mail addresses qualify; anything else is refused before a dialog is shown.
 */

export type ExternalLink =
  | {
      kind: 'web'
      /** The address exactly as the app gave it. */
      url: string
      /** `host[:port]` of the parsed URL. */
      host: string
    }
  | {
      kind: 'mail'
      url: string
      /** The recipient, decoded. */
      host: string
    }

// eslint-disable-next-line no-control-regex -- control characters are exactly what is refused
const CONTROL = /[\u0000-\u001f\u007f]/

/** Describe `href`, or `null` when it is not a web or mail link. */
export function describeLink(href: unknown): ExternalLink | null {
  if (typeof href !== 'string') return null
  const url = href.trim()
  if (url === '' || CONTROL.test(url)) return null
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return null
  }
  if (parsed.protocol === 'http:' || parsed.protocol === 'https:')
    return parsed.host ? { kind: 'web', url, host: parsed.host } : null
  if (parsed.protocol === 'mailto:') {
    const recipient = decodeRecipient(parsed.pathname)
    return recipient ? { kind: 'mail', url, host: recipient } : null
  }
  return null
}

function decodeRecipient(path: string): string {
  try {
    return decodeURIComponent(path).trim()
  } catch {
    return path.trim()
  }
}
