/**
 * Defines a custom element once. `kit.js` may be loaded twice (the plugin's tag and an app's own, a hot reload), and the
 * second `customElements.define` of a name throws; the first definition keeps working.
 */
export function define(tag: string, constructor: CustomElementConstructor): void {
  if (!customElements.get(tag)) customElements.define(tag, constructor)
}
