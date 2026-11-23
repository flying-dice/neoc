/**
 * What a story needs to reach inside a kit element. The testing library's queries stop at a shadow root, so a story that
 * answers a `ui-alert` asks here for the button to press.
 */

/** The button of a `ui-alert` that answers it: `cancel`, or `action` (the answer the alert was raised for). */
export function alertButton(alert: HTMLElement, answer: 'cancel' | 'action'): HTMLElement {
  const button = alert.shadowRoot?.querySelector<HTMLElement>(`[part=${answer}]`)
  if (!button) throw new Error(`no ${answer} button: is this element an open ui-alert?`)
  return button
}
