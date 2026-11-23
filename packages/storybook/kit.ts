/**
 * What a story needs to reach inside a kit element. The testing library's queries stop at a shadow root, so a story
 * that answers a `ui-alert`, reads what it asks, or types into a field asks here for the element inside.
 */

const inside = <T extends HTMLElement>(host: HTMLElement, selector: string, what: string): T => {
  const found = host.shadowRoot?.querySelector<T>(selector)
  if (!found) throw new Error(`no ${what} inside <${host.localName}>`)
  return found
}

/** The dialog of a `ui-alert`: what it asks (its heading and message) is this element's text. */
export const alertDialog = (alert: HTMLElement): HTMLDialogElement =>
  inside(alert, 'dialog', 'dialog')

/** The textarea a `ui-text-area` wraps: what a story types into and reads the value of. */
export const textArea = (area: HTMLElement): HTMLTextAreaElement =>
  inside(area, 'textarea', 'textarea')

/** The select a `ui-picker` wraps: what a story chooses an option of. */
export const pickerSelect = (picker: HTMLElement): HTMLSelectElement =>
  inside(picker, 'select', 'select')

/** The button of a `ui-alert` that answers it: `cancel`, or `action` (the answer the alert was raised for). */
export function alertButton(alert: HTMLElement, answer: 'cancel' | 'action'): HTMLElement {
  return inside(alert, `[part=${answer}]`, `${answer} button`)
}
