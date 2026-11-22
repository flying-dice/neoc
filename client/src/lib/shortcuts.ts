/**
 * The shell's own keyboard shortcuts. Pure: the page that listens passes the event and the platform.
 */

type Chord = {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  repeat?: boolean
  isComposing?: boolean
}

/**
 * System search: Cmd+K on a Mac, Ctrl+K anywhere else (a web page cannot have Cmd+Space, which is the Mac's own
 * Spotlight). Only that chord: with Shift or Alt, or with the other platform's modifier, it is somebody else's
 * (Ctrl+K in a text field on a Mac deletes to the end of the line).
 */
export function isSearchShortcut(e: Chord, mac: boolean): boolean {
  if (e.key.toLowerCase() !== 'k' || e.altKey || e.shiftKey || e.repeat || e.isComposing)
    return false
  return mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
}

/** `aria-keyshortcuts` for it. */
export const searchShortcut = (mac: boolean): string => (mac ? 'Meta+K' : 'Control+K')

/** Whether this is an Apple platform, where shortcuts are made with Cmd. */
export function isApplePlatform(
  nav:
    { platform?: string; userAgentData?: { platform?: string } } | undefined = typeof navigator ===
  'undefined'
    ? undefined
    : (navigator as { platform?: string; userAgentData?: { platform?: string } })
): boolean {
  const platform = nav?.userAgentData?.platform || nav?.platform || ''
  return /mac|iphone|ipad|ipod|ios/i.test(platform)
}
