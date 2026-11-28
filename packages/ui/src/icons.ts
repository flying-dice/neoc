/**
 * A small SF Symbols–inspired glyph set (24×24, stroked).
 * Each entry is the inner SVG markup. Add icons here; the `IconName`
 * type updates automatically.
 */
export const icons = {
  'chevron.right': '<path d="M9 5l7 7-7 7"/>',
  'chevron.left': '<path d="M15 5l-7 7 7 7"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  xmark: '<path d="M6 6l12 12M18 6L6 18"/>',
  checkmark: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  ellipsis:
    '<circle cx="5.5" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
  magnifyingglass: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.5 15.5L20 20"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  sparkles:
    '<path d="M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z"/><path d="M18 14l.8 2.2L21 17l-2.2.8L18 20l-.8-2.2L15 17l2.2-.8z"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M7 9l3 3-3 3M12.5 15H17"/>',
  folder: '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z"/>',
  doc: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h6"/>',
  'doc.on.doc': '<rect x="8.5" y="8.5" width="11" height="12" rx="2"/><path d="M15.5 8.5V6A2 2 0 0 0 13.5 4h-7a2 2 0 0 0-2 2v9.5a2 2 0 0 0 2 2h2"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3z"/>',
  message: '<path d="M21 11.5c0 4.14-4.03 7.5-9 7.5-1.1 0-2.15-.16-3.12-.46L4 20l1.3-3.6C3.87 15.07 3 13.36 3 11.5 3 7.36 7.03 4 12 4s9 3.36 9 7.5z"/>',
  'square.grid': '<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>',
  house: '<path d="M4 10.5L12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-6h-5v6H5a1 1 0 0 1-1-1z"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  cpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9.5" y="9.5" width="5" height="5" rx="1"/><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/>',
  bolt: '<path d="M13 3L5 13.5h6L10 21l8-10.5h-6z"/>',
  bell: '<path d="M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15z"/><path d="M10 20.5a2 2 0 0 0 4 0"/>',
  person: '<circle cx="12" cy="8" r="4"/><path d="M4.5 20c.8-3.7 3.9-6 7.5-6s6.7 2.3 7.5 6"/>',
  lock: '<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/>',
  trash: '<path d="M4 7h16M10 3.5h4M6 7l1 12.5A1.5 1.5 0 0 0 8.5 21h7a1.5 1.5 0 0 0 1.5-1.5L18 7M10 11v6M14 11v6"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>',
  play: '<path d="M7 4.5v15l12.5-7.5z" fill="currentColor"/>',
  pause: '<rect x="6.5" y="5" width="3.5" height="14" rx="1" fill="currentColor"/><rect x="14" y="5" width="3.5" height="14" rx="1" fill="currentColor"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>',
  'arrow.up': '<path d="M12 20V5M5.5 11.5L12 5l6.5 6.5"/>',
  sidebar: '<rect x="3" y="4.5" width="18" height="15" rx="3"/><path d="M9 4.5v15"/>',
  wifi: '<path d="M2.5 9.5a14 14 0 0 1 19 0M5.8 12.9a9.3 9.3 0 0 1 12.4 0M9.1 16.3a4.6 4.6 0 0 1 5.8 0"/><circle cx="12" cy="19.5" r="1" fill="currentColor" stroke="none"/>',
  'info.circle': '<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><circle cx="12" cy="7.75" r="1" fill="currentColor" stroke="none"/>',
  'exclamationmark.triangle': '<path d="M10.3 4.2L2.8 17.5A2 2 0 0 0 4.5 20.5h15a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4.5"/><circle cx="12" cy="17" r="1" fill="currentColor" stroke="none"/>',
  'square.and.pencil': '<path d="M11 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5"/><path d="M17.5 3.5a2.1 2.1 0 0 1 3 3L12 15l-4 1 1-4z"/>',
  network: '<circle cx="12" cy="5" r="2.5"/><circle cx="5" cy="19" r="2.5"/><circle cx="19" cy="19" r="2.5"/><path d="M12 7.5v4M12 11.5l-5.5 5.5M12 11.5l5.5 5.5"/>',
  star: '<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/>',
  'square.and.arrow.up': '<path d="M8 9H6.5A1.5 1.5 0 0 0 5 10.5v8A1.5 1.5 0 0 0 6.5 20h11a1.5 1.5 0 0 0 1.5-1.5v-8A1.5 1.5 0 0 0 17.5 9H16"/><path d="M12 14V3.5M8.5 7L12 3.5 15.5 7"/>',
  'rectangle.stack': '<rect x="3" y="9" width="18" height="11" rx="2.5"/><path d="M5.5 6h13M8 3h8"/>',
  calculator:
    '<rect x="5" y="3" width="14" height="18" rx="2.5"/><rect x="8" y="6" width="8" height="3.5" rx="1"/><circle cx="8.5" cy="13.2" r="1" fill="currentColor" stroke="none"/><circle cx="12" cy="13.2" r="1" fill="currentColor" stroke="none"/><circle cx="15.5" cy="13.2" r="1" fill="currentColor" stroke="none"/><circle cx="8.5" cy="17" r="1" fill="currentColor" stroke="none"/><circle cx="12" cy="17" r="1" fill="currentColor" stroke="none"/><circle cx="15.5" cy="17" r="1" fill="currentColor" stroke="none"/>',
  minus: '<path d="M5 12h14"/>',
  flag: '<path d="M6 21V4"/><path d="M6 4.5h11.5l-2.5 4 2.5 4H6"/>',
  'layout.fullscreen': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="5" y="7.5" width="14" height="9" rx="1" fill="currentColor" stroke="none"/>',
  'layout.split.left': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="5" y="7.5" width="6" height="9" rx="1" fill="currentColor" stroke="none"/><rect x="13" y="7.5" width="6" height="9" rx="1" opacity="0.35" fill="currentColor" stroke="none"/>',
  'layout.split.right': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="5" y="7.5" width="6" height="9" rx="1" opacity="0.35" fill="currentColor" stroke="none"/><rect x="13" y="7.5" width="6" height="9" rx="1" fill="currentColor" stroke="none"/>',
  'layout.slideover': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="14" y="7.5" width="5" height="9" rx="1" fill="currentColor" stroke="none"/>',
  'layout.window': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="7" y="8.5" width="10" height="7" rx="1" fill="currentColor" stroke="none"/>',
  'arrow.up.forward.square': '<path d="M12 4H7a3 3 0 0 0-3 3v10a3 3 0 0 0 3 3h10a3 3 0 0 0 3-3v-5"/><path d="M15 4h5v5M20 4l-8.5 8.5"/>',
  'dock.rectangle': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="6" y="13" width="12" height="3.5" rx="1.2" fill="currentColor" stroke="none"/>',
  'dock.rectangle.slash': '<rect x="2.5" y="5" width="19" height="14" rx="2.5"/><rect x="6" y="13" width="12" height="3.5" rx="1.2" fill="currentColor" stroke="none"/><path d="M4 3l16 18"/>',
} as const

export type IconName = keyof typeof icons
