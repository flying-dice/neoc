/**
 * What the person last did that bears on what Enter means on a focused control: walked to it with Tab (`keyboard`),
 * tapped or clicked it (`pointer`), or typed since (`typing`: a character, Backspace or Delete).
 *
 * `:focus-visible` cannot say. In Chromium a key pressed while a mouse-focused button has focus turns it on, so
 * "does this button show a focus ring" is true after a click as soon as the person types. What distinguishes a key
 * the person walked to from a key they tapped is the input that came before the focus: a Tab, or a pointer. And a
 * person who tabbed to a key and then types a sum is typing, not navigating: Enter finishes the sum.
 */

export type Modality = 'keyboard' | 'pointer' | 'typing'

let modality: Modality = 'keyboard'
let watching = 0
let stop: (() => void) | undefined

/** The way the last focus move happened. (A page that has seen no input yet counts as the keyboard.) */
export function inputModality(): Modality {
  return modality
}

type InputEvent = Pick<Event, 'type'> &
  Partial<Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey'>>

/**
 * Record the modality the event implies. Enter, Space and the arrows leave it alone: they press or move, they do not
 * type.
 */
export function noteInput(e: InputEvent): void {
  if (e.type === 'pointerdown') modality = 'pointer'
  else if (e.type !== 'keydown' || !e.key) return
  else if (e.key === 'Tab') modality = 'keyboard'
  else if (!e.metaKey && !e.ctrlKey && !e.altKey && isTyped(e.key)) modality = 'typing'
}

/** A printable character (not Space), Backspace or Delete: the keys a sum is typed with. Escape dismisses, it does not type. */
function isTyped(key: string): boolean {
  return (key.length === 1 && key !== ' ') || key === 'Backspace' || key === 'Delete'
}

/**
 * Start listening on the document (reference counted, so every window can ask); returns the release.
 * Capture phase: it runs before any handler that stops the event.
 */
export function watchInputModality(doc: Document = document): () => void {
  if (watching++ === 0) {
    const listen = noteInput as EventListener
    doc.addEventListener('keydown', listen, true)
    doc.addEventListener('pointerdown', listen, true)
    stop = () => {
      doc.removeEventListener('keydown', listen, true)
      doc.removeEventListener('pointerdown', listen, true)
    }
  }
  let released = false
  return () => {
    if (released) return
    released = true
    if (--watching === 0) {
      stop?.()
      stop = undefined
    }
  }
}

/** The elements that Enter and Space press when they have focus. */
const PRESSABLE = 'button, [role="button"], a[href], summary'

/**
 * Whether this Enter or Space is the press of the focused control (the browser does that itself) rather than a
 * window shortcut: the control is a button or link, the person walked to it with Tab (and has not typed since), and no
 * modifier is held. A key the person clicked is not walked to, and one they tabbed to and then typed on is not being
 * navigated, so Enter after tapping or typing still reaches a shortcut such as "=".
 */
export function pressesFocusedControl(
  e: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>,
  target: Element | null,
  how: Modality = modality
): boolean {
  if (e.key !== 'Enter' && e.key !== ' ') return false
  if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return false
  return how === 'keyboard' && !!target?.matches?.(PRESSABLE)
}
