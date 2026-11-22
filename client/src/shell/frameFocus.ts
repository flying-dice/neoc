/**
 * Give an app's frame the keyboard once it is ready: the one rule both host pages use (the shell's window, `AppFrame`, and the
 * standalone page, `wrapper/main.svelte.ts`), so opening an app by either route lets a person type at once.
 *
 * Focus that is already somewhere meaningful (a field, a dialog, another window) stays put, so a window an agent opens never takes the
 * keys from what the person is typing; a launcher (the Dock) or nothing at all does not count as somewhere.
 */
export function focusFrame(
  frame: { contentWindow: { focus(): void } | null },
  doc: { activeElement: Element | null; body: unknown } = document
): boolean {
  const active = doc.activeElement
  if (active && active !== doc.body && !active.closest('[data-launcher]')) return false
  frame.contentWindow?.focus()
  return true
}
