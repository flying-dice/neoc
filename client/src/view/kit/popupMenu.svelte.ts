/**
 * Open state of a menu opened from a trigger button (the Menu node, a menu-style Picker): toggled by click,
 * Enter, Space or ArrowDown, closed by selecting, Escape, Tab or a press outside, with focus returned to the
 * trigger. The panel is a kit `Popover` (kind `menu`) holding a kit `Menu`.
 */
// The returned object's type is inferred from its getters and setters.
// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
export function popupMenu() {
  let open = $state(false)
  let trigger: HTMLElement | undefined = $state()

  function close(restoreFocus = true): void {
    open = false
    if (restoreFocus) queueMicrotask(() => trigger?.focus({ preventScroll: true }))
  }

  return {
    get open() {
      return open
    },
    set open(v: boolean) {
      open = v
    },
    get trigger() {
      return trigger
    },
    set trigger(el: HTMLElement | undefined) {
      trigger = el
    },
    toggle(): void {
      open = !open
    },
    close,
    /** ArrowDown on the trigger opens the menu (the keyboard path alongside Enter and Space). */
    onTriggerKey(e: KeyboardEvent): void {
      if (e.key === 'ArrowDown' && !open) {
        e.preventDefault()
        open = true
      }
    }
  }
}
